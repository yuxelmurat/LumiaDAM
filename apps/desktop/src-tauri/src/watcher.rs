use anyhow::Result;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::Ordering;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::db::{make_relative_path, is_legacy_thumbnail, Database};
use crate::models::AssetKind;

struct PendingAsset {
    id: String,
    file_path: String,
    file_name: String,
    extension: String,
    kind_str: &'static str,
    size_bytes: i64,
    modified_at: i64,
    now: i64,
    preview_status: &'static str,
    thumbnail_path: Option<String>,
    needs_job: bool,
    is_update: bool,
}

/// Kaynak dosyaları okuyup yerel kataloğu günceller; kaynakta veri veya kilit oluşturmaz.
pub async fn index_paths(db: &Database, library_id: &str, paths: &[String]) -> Result<()> {
    let handle = db.get_library_handle(Some(library_id)).await
        .ok_or_else(|| anyhow::anyhow!("Library catalog is not open"))?;
    let _scan = handle.scan_lock.lock().await;
    handle.scan_processed.store(0, Ordering::Relaxed);
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(format!("scan_error:{library_id}"))
        .execute(&db.master_pool).await?;
    let pool = handle.pool;
    let root_path = handle.root_path;
    let thumbs_dir = handle.catalog_dir.join("thumbs");
    anyhow::ensure!(!paths.is_empty(), "No source folders to scan");
    for root in paths {
        anyhow::ensure!(Path::new(root).is_dir(), "Source is unavailable; the catalog is preserved: {}", root);
    }
    let now = Utc::now().timestamp_millis();

    // ── 1. Load existing assets in ONE single query into memory ──────────
    let existing_rows: Vec<(String, String, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, file_path, modified_at, size_bytes, thumbnail_path FROM assets"
    )
    .fetch_all(&pool)
    .await?;

    let mut existing_map: HashMap<String, (String, i64, i64, Option<String>)> = HashMap::with_capacity(existing_rows.len());
    for (id, file_path, modified_at, size_bytes, thumb) in existing_rows {
        // Normalize path separators to forward slash for reliable matching
        let norm = file_path.replace('\\', "/");
        existing_map.insert(norm, (id, modified_at, size_bytes, thumb));
    }

    let mut seen_paths: HashSet<String> = HashSet::with_capacity(existing_map.len() + 100);
    let mut seen_folders = HashSet::new();
    let mut pending_batch: Vec<PendingAsset> = Vec::with_capacity(200);
    // Tüm dosya türleri görünür; uygulamanın kendi ana veritabanı yine kaynak sayılmaz.
    let master_path = db.master_pool.connect_options().get_filename().to_string_lossy().into_owned();
    let internal_files: Vec<_> = ["", "-journal", "-wal", "-shm"].iter()
        .map(|suffix| std::path::PathBuf::from(format!("{master_path}{suffix}"))).collect();

    // ── 2. Walk directory tree ──────────────────────────────────────────
    for root in paths {
        let root_dir = Path::new(root);
        let walker = WalkDir::new(root_dir)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                // Yalnız uygulamanın katalog klasörlerini tarama dışında tut.
                if crate::storage::path_is_within(e.path(), &db.storage_root)
                    || internal_files.iter().any(|path| crate::storage::path_is_within(e.path(), path))
                    || crate::storage::path_is_within(e.path(), &db.default_storage_root)
                    || (e.file_type().is_dir() && e.path().join(".lumiadam-catalog").exists()) {
                    return false;
                }
                let file_name = e.file_name().to_string_lossy();
                if e.file_type().is_dir() && file_name == ".opendam" {
                    return false;
                }
                true
            });

        for entry in walker {
            // Eksik/başarısız tarama silme aşamasına ulaşmamalı.
            let entry = entry?;
            if entry.file_type().is_dir() {
                seen_folders.insert(make_relative_path(&root_path, entry.path()));
            }
            if !entry.file_type().is_file() { continue; }
            handle.scan_processed.fetch_add(1, Ordering::Relaxed);
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let rel_path = make_relative_path(&root_path, path);
            seen_paths.insert(rel_path.clone());

            let meta = entry.metadata()?;
            let size_bytes = meta.len() as i64;
            let modified_at = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(now);

            // In-memory cache check: if unchanged, SKIP completely!
            if let Some((_, cached_mod, cached_size, _)) = existing_map.get(&rel_path) {
                if *cached_mod == modified_at && *cached_size == size_bytes {
                    continue; // 0 disk writes, 0 SQL queries!
                }
            }

            let kind = AssetKind::from_extension(&ext);
            let kind_str = kind.as_str();

            let (id, is_update) = if let Some((existing_id, _, _, _)) = existing_map.get(&rel_path) {
                (existing_id.clone(), true)
            } else {
                (Uuid::new_v4().to_string(), false)
            };

            // Aynı adlı dosyaların önizlemeleri ayrı klasörlerde tutulur.
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&file_name);
            let asset_thumbs_dir = thumbs_dir.join(&id);
            let thumb_png = asset_thumbs_dir.join(format!("{}_thumb.png", stem));
            let thumb_jpg = asset_thumbs_dir.join(format!("{}_thumb.jpg", stem));

            let mut preview_status = "none";
            let mut thumbnail_path = None;
            let mut needs_job = false;
            let needs_preview = matches!(
                ext.as_str(),
                "max" | "fbx" | "obj" | "gltf" | "glb" | "dae" | "stl" | "3ds" | "skp" | "rfa" | "rvt" | "dwg" | "3dm" | "vrmat" | "psd" | "psb" | "ai" | "exr" | "hdr"
                | "jpg" | "jpeg" | "png" | "tif" | "tiff" | "tga" | "bmp" | "webp"
            );

            if needs_preview && thumb_png.exists() {
                preview_status = "done";
                thumbnail_path = Some(thumb_png.to_string_lossy().to_string());
            } else if needs_preview && thumb_jpg.exists() {
                preview_status = "done";
                thumbnail_path = Some(thumb_jpg.to_string_lossy().to_string());
            } else if needs_preview {
                preview_status = "pending";
                needs_job = true;
            }

            if let Some((_, _, _, existing_thumb)) = existing_map.get(&rel_path) {
                if thumbnail_path.is_none() && existing_thumb.as_ref().is_some_and(|path| !is_legacy_thumbnail(path)) {
                    thumbnail_path = existing_thumb.clone();
                    preview_status = "done";
                    needs_job = false;
                }
            }

            pending_batch.push(PendingAsset {
                id,
                file_path: rel_path,
                file_name,
                extension: ext,
                kind_str,
                size_bytes,
                modified_at,
                now,
                preview_status,
                thumbnail_path,
                needs_job,
                is_update,
            });

            // Flush chunk if batch reaches 200 items
            if pending_batch.len() >= 200 {
                commit_asset_batch(&pool, &pending_batch).await?;
                pending_batch.clear();
            }
        }
    }

    // Flush any remaining assets
    if !pending_batch.is_empty() {
        commit_asset_batch(&pool, &pending_batch).await?;
        pending_batch.clear();
    }

    crate::folders::replace(&pool, &seen_folders).await?;

    // ── 3. Detect and remove deleted files in batch ───────────────────────
    let to_delete: Vec<String> = existing_map
        .into_iter()
        .filter(|(rel, _)| !seen_paths.contains(rel))
        .map(|(_, (id, _, _, _))| id)
        .collect();

    if !to_delete.is_empty() {
        for chunk in to_delete.chunks(200) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!("DELETE FROM assets WHERE id IN ({})", placeholders);
            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(id);
            }
            let _ = query.execute(&pool).await;
        }
    }

    // ── 4. Update asset count ───────────────────────────────────────────
    let count_row: Option<(i64,)> = sqlx::query_as("SELECT COUNT(*) FROM assets").fetch_optional(&pool).await?;
    let total_count = count_row.map(|(c,)| c).unwrap_or(0);

    let _ = sqlx::query("UPDATE libraries SET asset_count = ? WHERE id = ?")
        .bind(total_count)
        .bind(library_id)
        .execute(&pool)
        .await;

    let _ = sqlx::query("UPDATE libraries SET asset_count = ? WHERE id = ?")
        .bind(total_count)
        .bind(library_id)
        .execute(&db.master_pool)
        .await;

    sqlx::query("INSERT INTO settings(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(format!("last_scan:{library_id}"))
        .bind(Utc::now().timestamp_millis().to_string())
        .execute(&db.master_pool)
        .await?;

    tracing::info!("NAS-efficient indexing complete for library {}: {} total assets", library_id, total_count);
    Ok(())
}

/// Helper to execute a batch of assets inside a single database transaction
async fn commit_asset_batch(pool: &sqlx::Pool<sqlx::Sqlite>, batch: &[PendingAsset]) -> Result<()> {
    let mut tx = pool.begin().await?;

    for item in batch {
        if item.is_update {
            sqlx::query(
                r#"UPDATE assets SET
                    size_bytes = ?,
                    modified_at = ?,
                    preview_status = CASE WHEN preview_status = 'done' THEN 'done' ELSE ? END,
                    thumbnail_path = COALESCE(?, thumbnail_path)
                   WHERE id = ?"#
            )
            .bind(item.size_bytes)
            .bind(item.modified_at)
            .bind(item.preview_status)
            .bind(&item.thumbnail_path)
            .bind(&item.id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                r#"INSERT INTO assets
                   (id, file_path, file_name, extension, kind, size_bytes,
                    modified_at, indexed_at, rating, color_label, is_favorite, description,
                    thumbnail_path, preview_status, metadata)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 'none', 0, '', ?, ?, '{}')"#
            )
            .bind(&item.id)
            .bind(&item.file_path)
            .bind(&item.file_name)
            .bind(&item.extension)
            .bind(item.kind_str)
            .bind(item.size_bytes)
            .bind(item.modified_at)
            .bind(item.now)
            .bind(&item.thumbnail_path)
            .bind(item.preview_status)
            .execute(&mut *tx)
            .await?;

            if item.needs_job {
                let job_id = Uuid::new_v4().to_string();
                let _ = sqlx::query(
                    "INSERT INTO preview_jobs (id, asset_id, status, created_at) VALUES (?, ?, 'pending', ?)"
                )
                .bind(&job_id)
                .bind(&item.id)
                .bind(item.now)
                .execute(&mut *tx)
                .await;
            }
        }
    }

    tx.commit().await?;
    Ok(())
}

/// LibraryWatcher placeholder
#[derive(Default)]
pub struct LibraryWatcher {}

impl LibraryWatcher {
    pub fn new() -> Self {
        Self {}
    }
}
