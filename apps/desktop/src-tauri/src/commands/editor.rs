use crate::{
    db::{make_relative_path, resolve_path, LibraryHandle},
    models::AssetKind,
    AppState,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{
    ipc::{InvokeBody, Request, Response},
    Emitter, State,
};

const MAX_BYTES: usize = 256 * 1024 * 1024;
type Result<T> = std::result::Result<T, String>;

fn supported(extension: &str) -> bool {
    matches!(
        extension,
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "tif" | "tiff" | "pcraft"
    )
}

#[derive(Clone, Copy)]
enum Editor { Photo, Pdf, Vector, Cad }
impl Editor {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("photo") {
            "photo" => Ok(Self::Photo), "pdf" => Ok(Self::Pdf), "vector" => Ok(Self::Vector), "cad" => Ok(Self::Cad),
            _ => Err("Unknown editor".into()),
        }
    }
    fn name(self) -> &'static str {
        match self { Self::Photo => "PhotoCraft", Self::Pdf => "PdfCraft", Self::Vector => "VectorCraft", Self::Cad => "CAD Viewer" }
    }
    fn supports(self, extension: &str, output: bool) -> bool {
        match self {
            Self::Photo => supported(extension),
            Self::Pdf => extension == "pdf",
            Self::Vector => matches!(extension, "svg" | "svgz" | "pdf" | "vectorcraft" | "jpg" | "jpeg" | "png" | "webp") ||
                (!output && matches!(extension, "ai" | "drawcraft" | "bmp" | "tif" | "tiff" | "gif")),
            Self::Cad => !output && matches!(extension, "dwg" | "dxf"),
        }
    }
}

async fn source(state: &AppState, id: &str, editor: Editor) -> Result<(LibraryHandle, PathBuf)> {
    for library in state.db.get_all_libraries().await {
        let path: Option<(String,)> = sqlx::query_as("SELECT file_path FROM assets WHERE id = ?")
            .bind(id)
            .fetch_optional(&library.pool)
            .await
            .map_err(|e| e.to_string())?;
        if let Some((path,)) = path {
            let path = PathBuf::from(resolve_path(&library.root_path, &path));
            let canonical = path.canonicalize().map_err(|e| e.to_string())?;
            let root = library
                .root_path
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !canonical.starts_with(root) || !path.is_file() {
                return Err("Source is outside the library or unavailable".into());
            }
            let extension = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !editor.supports(&extension, false) {
                return Err("This format is not enabled in the editor yet".into());
            }
            return Ok((library, path));
        }
    }
    Err("Source asset not found in an open library".into())
}

#[tauri::command]
pub async fn read_editor_asset(id: String, editor: Option<String>, state: State<'_, Arc<AppState>>) -> Result<Response> {
    let editor = Editor::parse(editor.as_deref())?;
    let (_, path) = source(&state, &id, editor).await?;
    let bytes = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err(format!("{} supports files up to 256 MB", editor.name()));
        }
        Ok::<Vec<u8>, String>(bytes)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Response::new(bytes))
}

fn header<'a>(request: &'a Request<'_>, name: &str) -> Result<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|s| s.to_str().ok())
        .ok_or_else(|| format!("Missing {name}"))
}

// Yeniden denenen kayıt aynı çıktıyı kullanır; var olan hiçbir dosyanın üzerine yazılmaz.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
                drop(file);
                let cleanup = std::fs::remove_file(path);
                return Err(format!(
                    "Could not save export: {error}; partial-file cleanup: {cleanup:?}"
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::metadata(path).map_err(|e| e.to_string())?.len() == bytes.len() as u64
                && std::fs::read(path).map_err(|e| e.to_string())? == bytes
            {
                Ok(())
            } else {
                Err(
                    "Export path already exists with different contents; no file was overwritten"
                        .into(),
                )
            }
        }
        Err(error) => Err(error.to_string()),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    file_path: String,
    asset_id: String,
}

#[tauri::command]
pub async fn save_editor_export(
    request: Request<'_>,
    state: State<'_, Arc<AppState>>,
) -> Result<ExportResult> {
    let source_id = header(&request, "x-source-asset")?.to_string();
    let export_id = uuid::Uuid::parse_str(header(&request, "x-export-id")?)
        .map_err(|e| e.to_string())?
        .to_string();
    let extension = header(&request, "x-export-extension")?.to_ascii_lowercase();
    let editor = Editor::parse(request.headers().get("x-editor-kind").and_then(|v| v.to_str().ok()))?;
    if !editor.supports(&extension, true) {
        return Err(format!("This output format is not enabled for {}", editor.name()));
    }
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes,
        _ => return Err("Expected binary export".into()),
    };
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("Export must be between 1 byte and 256 MB".into());
    }
    let (library, source_path) = source(&state, &source_id, editor).await?;
    let _scan = library.scan_lock.lock().await;
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Invalid source name")?;
    let file_name = format!("{stem}_edited_{export_id}.{extension}");
    let path = source_path.with_file_name(&file_name);
    let write_path = path.clone();
    let data = bytes.clone();
    tokio::task::spawn_blocking(move || write_new(&write_path, &data))
        .await
        .map_err(|e| e.to_string())??;
    let now = chrono::Utc::now().timestamp_millis();
    let modified = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|t| t.as_millis() as i64)
        .unwrap_or(now);
    let relative = make_relative_path(&library.root_path, &path);
    let metadata = serde_json::json!({"source_asset_id": source_id, "editor": editor.name(), "editor_export_id": export_id}).to_string();
    // Kaydı ve küçük resim işini tek işlemde ekler; hata olursa çıktı diskte korunur ve Retry ile tamamlanır.
    let index = async {
        let mut tx = library.pool.begin().await?;
        sqlx::query("INSERT INTO assets (id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at,metadata,preview_status) VALUES (?,?,?,?,?,?,?,?,?,'pending') ON CONFLICT(file_path) DO UPDATE SET metadata=excluded.metadata")
            .bind(&export_id).bind(&relative).bind(&file_name).bind(&extension).bind(AssetKind::from_extension(&extension).as_str())
            .bind(bytes.len() as i64).bind(modified).bind(now).bind(metadata).execute(&mut *tx).await?;
        let (asset_id,): (String,) = sqlx::query_as("SELECT id FROM assets WHERE file_path=?").bind(&relative).fetch_one(&mut *tx).await?;
        if !matches!(extension.as_str(), "pcraft" | "vectorcraft" | "svgz") {
            sqlx::query("INSERT INTO preview_jobs (id,asset_id,status,created_at) VALUES (?,?,'pending',?) ON CONFLICT(id) DO NOTHING")
                .bind(&export_id).bind(&asset_id).bind(now).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE assets SET preview_status='none' WHERE id=?").bind(&asset_id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok::<String, sqlx::Error>(asset_id)
    }.await.map_err(|e| format!("Export saved at {} but catalog update failed: {e}. Keep the editor open and retry.", path.display()))?;
    let _ = state.app_handle.emit("library:updated", &library.id);
    Ok(ExportResult {
        file_path: path.to_string_lossy().into_owned(),
        asset_id: index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_does_not_overwrite_existing_contents() {
        let path =
            std::env::temp_dir().join(format!("lumiadam-export-{}.png", uuid::Uuid::new_v4()));
        write_new(&path, b"first").unwrap();
        write_new(&path, b"first").unwrap();
        assert!(write_new(&path, b"other").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn export_extension_rejects_paths_and_executables() {
        for extension in ["../png", "exe", "png/../../file", "", "zip"] {
            assert!(!supported(extension));
        }
        for extension in ["jpg", "png", "pcraft", "tiff"] {
            assert!(supported(extension));
        }
    }
    #[test]
    fn editor_formats_are_scoped_and_unknown_editors_are_rejected() {
        assert!(Editor::parse(Some("unknown")).is_err());
        assert!(Editor::Pdf.supports("pdf", false));
        assert!(!Editor::Pdf.supports("jpg", false));
        assert!(Editor::Vector.supports("svg", false));
        assert!(Editor::Vector.supports("ai", false));
        assert!(Editor::Vector.supports("vectorcraft", true));
        assert!(!Editor::Vector.supports("ai", true));
        assert!(Editor::Cad.supports("dwg", false));
        assert!(Editor::Cad.supports("dxf", false));
        assert!(!Editor::Cad.supports("dwg", true));
        for editor in [Editor::Photo, Editor::Pdf, Editor::Vector, Editor::Cad] {
            for ext in ["exe", "../pdf", "zip", "", "png/../../file"] {
                assert!(!editor.supports(ext, false));
                assert!(!editor.supports(ext, true));
            }
        }
        assert!(Editor::parse(None).unwrap().supports("png", false));
    }
}
