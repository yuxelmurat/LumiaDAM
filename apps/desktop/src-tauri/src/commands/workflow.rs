use crate::{
    workflow::{self, PathLabels},
    AppState,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
};
use tauri::{Emitter, State};
type Result<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_path_facets(
    library_ids: Option<Vec<String>>,
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, Vec<String>>> {
    let mut facets: HashMap<String, HashSet<String>> = HashMap::new();
    for library in state.db.get_all_libraries().await {
        if library_ids
            .as_ref()
            .is_some_and(|ids| !ids.is_empty() && !ids.contains(&library.id))
        {
            continue;
        }
        for field in ["year", "source", "room", "renderer"] {
            let rows: Vec<String> = sqlx::query_scalar(&format!(
                "SELECT DISTINCT {field} FROM asset_workflow WHERE {field}<>''"
            ))
            .fetch_all(&library.pool)
            .await
            .map_err(|e| e.to_string())?;
            facets.entry(field.into()).or_default().extend(rows);
        }
    }
    Ok(facets
        .into_iter()
        .map(|(key, values)| {
            let mut values: Vec<_> = values.into_iter().collect();
            values.sort();
            (key, values)
        })
        .collect())
}

#[tauri::command]
pub async fn set_path_labels(
    id: String,
    labels: Option<PathLabels>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let (library, path, _, _, _) = workflow::locate(&state.db, &id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(labels) = &labels {
        for value in [&labels.year, &labels.source, &labels.room, &labels.renderer] {
            if value.len() > 120 || value.chars().any(char::is_control) {
                return Err(
                    "Labels must be at most 120 bytes and contain no control characters".into(),
                );
            }
        }
    }
    let overrides = labels
        .map(|labels| serde_json::to_string(&labels).unwrap())
        .unwrap_or_else(|| "{}".into());
    let mut tx = library.pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("UPDATE asset_workflow SET overrides=? WHERE asset_id=?")
        .bind(overrides)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let (ext, kind): (String, String) =
        sqlx::query_as("SELECT extension,kind FROM assets WHERE id=?")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    workflow::upsert(&mut tx, &id, &path, &ext, &kind)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn set_related_image(
    id: String,
    image_id: Option<String>,
    mode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    link_image(&state.db, id, image_id, mode).await
}

pub(crate) async fn link_image(
    db: &crate::db::Database,
    id: String,
    image_id: Option<String>,
    mode: String,
) -> Result<()> {
    if !matches!(mode.as_str(), "auto" | "pinned" | "none") {
        return Err("Invalid image relation mode".into());
    }
    let (library, path, _, _, _) = workflow::locate(db, &id).await.map_err(|e| e.to_string())?;
    if mode == "pinned" {
        let image: Option<(String, String)> =
            sqlx::query_as("SELECT file_path,extension FROM assets WHERE id=?")
                .bind(&image_id)
                .fetch_optional(&library.pool)
                .await
                .map_err(|e| e.to_string())?;
        let Some((image_path, ext)) = image else {
            return Err("Select a catalog image from the same folder".into());
        };
        let directory = |path: &str| {
            path.replace('\\', "/")
                .rsplit_once('/')
                .map(|(dir, _)| dir.to_lowercase())
                .unwrap_or_default()
        };
        if id == image_id.clone().unwrap_or_default()
            || directory(&image_path) != directory(&path)
            || !matches!(ext.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp")
        {
            return Err(
                "Related images must be JPG, PNG or WebP files from the same library folder".into(),
            );
        }
    }
    sqlx::query("UPDATE asset_workflow SET related_mode=?,related_id=? WHERE asset_id=?")
        .bind(&mode)
        .bind(if mode == "pinned" { image_id } else { None })
        .bind(id)
        .execute(&library.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_related_images(
    id: String,
    text: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<workflow::GroupFile>> {
    let (library, _, _, _, _) = workflow::locate(&state.db, &id)
        .await
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String,String,String,String,String,Option<String>)>=sqlx::query_as("SELECT a.id,a.file_name,a.file_path,a.extension,a.kind,a.thumbnail_path FROM assets a JOIN asset_workflow w ON w.asset_id=a.id WHERE w.directory=(SELECT directory FROM asset_workflow WHERE asset_id=?) AND lower(a.extension) IN ('jpg','jpeg','png','webp') AND a.id<>? AND instr(lower(a.file_name),lower(?))>0 ORDER BY a.file_name COLLATE NOCASE LIMIT 200")
        .bind(&id).bind(&id).bind(text.unwrap_or_default()).fetch_all(&library.pool).await.map_err(|e|e.to_string())?;
    Ok(rows
        .into_iter()
        .map(
            |(id, name, path, extension, kind, thumb)| workflow::GroupFile {
                id,
                file_name: name,
                file_path: crate::db::resolve_path(&library.root_path, &path),
                extension,
                kind,
                thumbnail_path: thumb
                    .filter(|p| !crate::db::is_legacy_thumbnail(p))
                    .map(|p| crate::db::resolve_path(&library.root_path, &p)),
            },
        )
        .collect())
}

#[tauri::command]
pub async fn resolve_asset_file(id: String, state: State<'_, Arc<AppState>>) -> Result<String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(8),
        workflow::readable_path(&state.db, &id),
    )
    .await
    .map_err(|_| {
        "The source did not respond. Reconnect the library or use an offline copy.".to_string()
    })?
    .map(|path| path.to_string_lossy().into_owned())
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Availability {
    source_available: bool,
    offline_available: bool,
    offline_stale: bool,
}

#[tauri::command]
pub async fn check_asset_availability(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Availability> {
    let (library, path, _, size, modified) = workflow::locate(&state.db, &id)
        .await
        .map_err(|e| e.to_string())?;
    let cached: Option<(String, i64, i64)> = sqlx::query_as(
        "SELECT file_name,size_bytes,modified_at FROM offline_assets WHERE asset_id=?",
    )
    .bind(&id)
    .fetch_optional(&library.pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut offline_available = false;
    let mut offline_stale = false;
    if let Some((name, cached_size, cached_modified)) = cached {
        offline_stale = size != cached_size || modified != cached_modified;
        let path = workflow::offline_path(&library, &id, &name).map_err(|e| e.to_string())?;
        offline_available = tokio::fs::metadata(path)
            .await
            .is_ok_and(|meta| meta.is_file() && meta.len() == cached_size as u64);
    }
    // Bu kontrol yalnız kullanıcı istediğinde NAS'a erişir.
    let source = crate::db::resolve_path(&library.root_path, &path);
    let source_available = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::fs::metadata(source),
    )
    .await
    .is_ok_and(|result| result.is_ok_and(|meta| meta.is_file()));
    Ok(Availability {
        source_available,
        offline_available,
        offline_stale,
    })
}

static OFFLINE_BUSY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
struct OfflineGuard(String);
impl OfflineGuard {
    fn start(id: &str) -> Result<Self> {
        let mut busy = OFFLINE_BUSY
            .get_or_init(Default::default)
            .lock()
            .map_err(|e| e.to_string())?;
        if !busy.insert(id.into()) {
            return Err("This file is already being updated in offline storage".into());
        }
        Ok(Self(id.into()))
    }
}
impl Drop for OfflineGuard {
    fn drop(&mut self) {
        if let Ok(mut busy) = OFFLINE_BUSY.get_or_init(Default::default).lock() {
            busy.remove(&self.0);
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyProgress {
    asset_id: String,
    bytes: u64,
    total: u64,
}

/// Kopya tamamlanıp kaynak doğrulanmadan katalogda çevrimdışı kullanılabilir olarak işaretlenmez.
#[tauri::command]
pub async fn keep_offline(id: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    let app = state.app_handle.clone();
    cache_file(
        &state.db,
        id,
        Some(Arc::new(move |progress| {
            let _ = app.emit("offline:progress", progress);
        })),
    )
    .await
}

pub(crate) async fn cache_file(
    db: &crate::db::Database,
    id: String,
    app: Option<Arc<dyn Fn(CopyProgress) + Send + Sync>>,
) -> Result<()> {
    let _guard = OfflineGuard::start(&id)?;
    let (library, relative, name, size, modified) =
        workflow::locate(db, &id).await.map_err(|e| e.to_string())?;
    let target = workflow::offline_path(&library, &id, &name).map_err(|e| e.to_string())?;
    let source = workflow::source_path(db, &library, &relative)
        .await
        .map_err(|e| e.to_string())?;
    let copy_id = id.clone();
    let returned = tokio::task::spawn_blocking(move || -> anyhow::Result<std::path::PathBuf> {
        use std::io::{Read, Write};
        let mut input = std::fs::File::open(&source)?;
        let before = input.metadata()?;
        let timestamp = |meta: &std::fs::Metadata| -> anyhow::Result<i64> {
            Ok(meta
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as i64)
        };
        anyhow::ensure!(
            before.is_file() && before.len() == size as u64 && timestamp(&before)? == modified,
            "Source has changed. Rescan the library before saving an offline copy."
        );
        std::fs::create_dir_all(target.parent().unwrap())?;
        let staging = target.with_file_name(format!(".copy-{}", uuid::Uuid::new_v4()));
        let result = (|| -> anyhow::Result<std::path::PathBuf> {
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)?;
            let mut buffer = vec![0u8; 128 * 1024];
            let mut copied = 0;
            let mut last = std::time::Instant::now();
            let _ = app.as_ref().map(|app| {
                app(CopyProgress {
                    asset_id: copy_id.clone(),
                    bytes: 0,
                    total: before.len(),
                })
            });
            loop {
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                output.write_all(&buffer[..read])?;
                copied += read as u64;
                if last.elapsed().as_millis() >= 250 {
                    let _ = app.as_ref().map(|app| {
                        app(CopyProgress {
                            asset_id: copy_id.clone(),
                            bytes: copied,
                            total: before.len(),
                        })
                    });
                    last = std::time::Instant::now();
                }
            }
            output.sync_all()?;
            drop(output);
            anyhow::ensure!(
                copied == before.len()
                    && timestamp(&input.metadata()?)? == modified
                    && timestamp(&std::fs::metadata(&source)?)? == modified,
                "Source changed during copying; the old offline copy is preserved"
            );
            // Güncellemeler farklı UUID adlı sürüme yazılır; eski kopya atomik katalog geçişine kadar korunur.
            let completed = target.with_file_name(format!(
                "{}-{}",
                uuid::Uuid::new_v4(),
                target.file_name().unwrap().to_string_lossy()
            ));
            std::fs::rename(&staging, &completed)?;
            let mut permissions = std::fs::metadata(&completed)?.permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&completed, permissions)?;
            let _ = app.as_ref().map(|app| {
                app(CopyProgress {
                    asset_id: copy_id.clone(),
                    bytes: copied,
                    total: before.len(),
                })
            });
            Ok(completed)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&staging);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let cached_name = returned.file_name().unwrap().to_string_lossy().into_owned();
    let old: Option<String> =
        sqlx::query_scalar("SELECT file_name FROM offline_assets WHERE asset_id=?")
            .bind(&id)
            .fetch_optional(&library.pool)
            .await
            .map_err(|e| e.to_string())?;
    let save = sqlx::query("INSERT INTO offline_assets(asset_id,file_name,size_bytes,modified_at,cached_at) VALUES(?,?,?,?,?) ON CONFLICT(asset_id) DO UPDATE SET file_name=excluded.file_name,size_bytes=excluded.size_bytes,modified_at=excluded.modified_at,cached_at=excluded.cached_at")
        .bind(&id).bind(&cached_name).bind(size).bind(modified).bind(chrono::Utc::now().timestamp_millis()).execute(&library.pool).await;
    if let Err(error) = save {
        let _ = remove_local_copy(&returned).await;
        return Err(error.to_string());
    }
    if let Some(old) = old {
        if let Ok(path) = workflow::offline_path(&library, &id, &old) {
            let _ = remove_local_copy(&path).await;
        }
    }
    sqlx::query("UPDATE preview_jobs SET error_message=NULL WHERE asset_id=? AND error_message='Local copy unavailable'").bind(&id).execute(&library.pool).await.map_err(|e|e.to_string())?;
    db.preview_wakeup.notify_waiters();
    Ok(())
}

#[tauri::command]
pub async fn remove_offline(id: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    let _guard = OfflineGuard::start(&id)?;
    let (library, _, _, _, _) = workflow::locate(&state.db, &id)
        .await
        .map_err(|e| e.to_string())?;
    let name: Option<String> =
        sqlx::query_scalar("SELECT file_name FROM offline_assets WHERE asset_id=?")
            .bind(&id)
            .fetch_optional(&library.pool)
            .await
            .map_err(|e| e.to_string())?;
    if let Some(name) = name {
        let path = workflow::offline_path(&library, &id, &name).map_err(|e| e.to_string())?;
        remove_local_copy(&path).await?;
        sqlx::query("DELETE FROM offline_assets WHERE asset_id=?")
            .bind(id)
            .execute(&library.pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn remove_local_copy(path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    if let Ok(metadata) = tokio::fs::metadata(path).await {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        tokio::fs::set_permissions(path, permissions)
            .await
            .map_err(|e| e.to_string())?;
    }
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn get_project_notes(
    collection_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT asset_id,note FROM project_notes WHERE collection_id=?")
            .bind(collection_id)
            .fetch_all(&state.db.master_pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().collect())
}

#[tauri::command]
pub async fn save_project_note(
    collection_id: String,
    asset_id: String,
    note: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    save_note(&state.db, collection_id, asset_id, note).await
}

pub(crate) async fn save_note(
    db: &crate::db::Database,
    collection_id: String,
    asset_id: String,
    note: String,
) -> Result<()> {
    if note.len() > 10_000 || note.contains('\0') {
        return Err("Project notes must be at most 10,000 bytes".into());
    }
    let regular: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM collections WHERE id=? AND is_smart=0)")
            .bind(&collection_id)
            .fetch_one(&db.master_pool)
            .await
            .map_err(|e| e.to_string())?;
    if !regular {
        return Err("Project not found or this is a smart collection".into());
    }
    let mut member = false;
    for lib in db.get_all_libraries().await {
        let exists: bool=sqlx::query_scalar("WITH RECURSIVE scope(id) AS (SELECT ? UNION SELECT c.id FROM collections c JOIN scope s ON c.parent_id=s.id) SELECT EXISTS(SELECT 1 FROM collection_assets WHERE collection_id IN (SELECT id FROM scope) AND asset_id=?)")
            .bind(&collection_id).bind(&asset_id).fetch_one(&lib.pool).await.map_err(|e| e.to_string())?;
        member |= exists;
    }
    if !member {
        return Err("Add this asset to the project before adding a note".into());
    }
    sqlx::query("INSERT INTO project_notes(collection_id,asset_id,note) VALUES(?,?,?) ON CONFLICT(collection_id,asset_id) DO UPDATE SET note=excluded.note")
        .bind(collection_id).bind(asset_id).bind(note).execute(&db.master_pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn update_project(
    id: String,
    description: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    if description.len() > 10_000 || description.contains('\0') {
        return Err("Project description is too long".into());
    }
    let result = sqlx::query("UPDATE collections SET description=? WHERE id=? AND is_smart=0")
        .bind(&description)
        .bind(&id)
        .execute(&state.db.master_pool)
        .await
        .map_err(|e| e.to_string())?;
    if result.rows_affected() == 0 {
        return Err("Project not found".into());
    }
    for lib in state.db.get_all_libraries().await {
        sqlx::query("UPDATE collections SET description=? WHERE id=?")
            .bind(&description)
            .bind(&id)
            .execute(&lib.pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn find_duplicates(
    mode: String,
    library_ids: Option<Vec<String>>,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::duplicates::DuplicateResult> {
    crate::duplicates::scan(&state, mode, library_ids)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn cancel_duplicate_scan() {
    crate::duplicates::cancel();
}
