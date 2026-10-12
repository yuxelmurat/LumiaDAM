use anyhow::{bail, Context, Result};
use sqlx::{sqlite::SqliteConnectOptions, Connection, SqliteConnection};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

pub fn path_is_within(path: &Path, parent: &Path) -> bool {
    let normalize = |path: &Path| {
        let value = path.to_string_lossy().replace('\\', "/");
        let value = value.strip_prefix("//?/").unwrap_or(&value).trim_end_matches('/').to_string();
        if cfg!(windows) { value.to_lowercase() } else { value }
    };
    let path = normalize(path);
    let parent = normalize(parent);
    path == parent || path.starts_with(&(parent + "/"))
}

pub fn validate_location(path: &Path, sources: &[PathBuf], catalogs: &[PathBuf]) -> Result<()> {
    use std::io::Write;
    anyhow::ensure!(path.is_absolute() && path.is_dir(), "Select an existing folder using an absolute path");
    let resolved = std::fs::canonicalize(path)?;
    for root in sources.iter().chain(catalogs) {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        anyhow::ensure!(!path_is_within(&resolved, &root), "Storage cannot be inside a source library or an existing catalog folder");
    }
    let probe = path.join(format!(".lumiadam-write-test-{}", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
        file.write_all(b"LumiaDAM")?;
        file.sync_all()?;
        Ok(())
    })();
    let _ = std::fs::remove_file(probe);
    result.context("Cannot write to the selected folder")
}

/// Açık veritabanını dosya kopyasıyla değil, tutarlı bir SQLite anlık görüntüsüyle aktarır.
pub async fn snapshot(source: &Path, target: &Path) -> Result<()> {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(source).read_only(true),
    ).await?;
    let result = sqlx::query("VACUUM INTO ?")
        .bind(target.to_string_lossy().as_ref()).execute(&mut connection).await;
    connection.close().await?;
    result?;
    Ok(())
}

pub fn catalog_name(name: &str, id: &str) -> Result<String> {
    let id = Uuid::parse_str(id)?;
    let name: String = name.chars().take(40)
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    Ok(format!("{}--{}", if name.is_empty() { "Library" } else { &name }, id))
}

fn relocated_preview(value: &str, source: &Path, target: &Path) -> Option<String> {
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    let relative = path.strip_prefix(".opendam").ok()
        .or_else(|| path.strip_prefix(source).ok())?;
    if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(target.join(relative).to_string_lossy().to_string())
}

/// Eski katalog korunur. Yalnız tamamlanmış kopya görünür hedefe yükseltilir.
pub async fn prepare_catalog(storage_root: &Path, name: &str, id: &str, source: Option<&Path>) -> Result<PathBuf> {
    std::fs::create_dir_all(storage_root)?;
    let base = catalog_name(name, id)?;
    let mut destination = storage_root.join(&base);
    // Önceki taşımadan kalan kopya varsa üzerine yazılmaz.
    if destination.try_exists()? {
        destination = storage_root.join(format!("{}--{}", base, Uuid::new_v4()));
    }
    let staging = storage_root.join(format!(".migration-{}", Uuid::new_v4()));
    std::fs::create_dir(&staging)?;
    let result: Result<()> = async {
        std::fs::create_dir(staging.join("thumbs"))?;
        std::fs::write(staging.join(".lumiadam-catalog"), id)?;
        if let Some(source) = source {
            snapshot(&source.join("library.sqlite"), &staging.join("library.sqlite")).await
                .context("Cannot read the old catalog; the source is preserved")?;
            let source_thumbs = source.join("thumbs");
            if source_thumbs.try_exists()? {
                for entry in walkdir::WalkDir::new(&source_thumbs).follow_links(false) {
                    let entry = entry?;
                    if entry.path_is_symlink() { bail!("Transfer stopped: the preview folder contains a symbolic link"); }
                    let target = staging.join("thumbs").join(entry.path().strip_prefix(&source_thumbs)?);
                    if entry.file_type().is_dir() {
                        std::fs::create_dir_all(target)?;
                    } else {
                        std::fs::copy(entry.path(), target)?;
                    }
                }
            }
            let mut connection = SqliteConnection::connect_with(
                &SqliteConnectOptions::new().filename(staging.join("library.sqlite")),
            ).await?;
            let rewrite: Result<()> = async {
                let mut transaction = connection.begin().await?;
                // VACUUM sonrasında değişebilen rowid bağlantılarını güncellemeden önce kur.
                sqlx::query("INSERT INTO assets_fts(assets_fts) VALUES('rebuild')").execute(&mut *transaction).await?;
                crate::search_index::rebuild_after_snapshot(&mut transaction).await?;
                let rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
                    "SELECT id, thumbnail_path, metadata FROM assets"
                ).fetch_all(&mut *transaction).await?;
                for (id, thumbnail, metadata) in rows {
                    let thumbnail = thumbnail.map(|v| relocated_preview(&v, source, &destination).unwrap_or(v));
                    let mut metadata: serde_json::Value = serde_json::from_str(&metadata)?;
                    if let Some(value) = metadata.get("preview_model").and_then(|v| v.as_str()) {
                        if let Some(path) = relocated_preview(value, source, &destination) {
                            metadata["preview_model"] = path.into();
                        }
                    }
                    sqlx::query("UPDATE assets SET thumbnail_path = ?, metadata = ? WHERE id = ?")
                        .bind(thumbnail).bind(metadata.to_string()).bind(id).execute(&mut *transaction).await?;
                }
                transaction.commit().await?;
                let check: String = sqlx::query_scalar("PRAGMA quick_check").fetch_one(&mut connection).await?;
                if check != "ok" { bail!("Catalog validation failed: {}", check); }
                Ok(())
            }.await;
            connection.close().await?;
            rewrite?;
        }
        std::fs::rename(&staging, &destination)?;
        Ok(())
    }.await;
    if result.is_err() {
        // Yalnız bu çağrının oluşturduğu geçici klasörü temizle.
        let _ = std::fs::remove_dir_all(&staging);
    }
    result?;
    Ok(destination)
}

#[cfg(test)]
mod tests;
