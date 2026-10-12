use anyhow::Result;
use chrono::Utc;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Emitter;
use tokio::sync::mpsc;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::db::{make_relative_path, Database, LibraryHandle};
use crate::models::AssetKind;
use crate::AppState;

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
    needs_job: bool,
    is_update: bool,
}

struct ScannedEntry {
    path: PathBuf,
    file: Option<(i64, i64)>,
}

fn path_key(path: &str) -> String {
    let path = path.replace('\\', "/");
    if cfg!(windows) { path.to_lowercase() } else { path }
}

fn excluded(path: &Path, internal: &[PathBuf]) -> bool {
    internal.iter().any(|parent| crate::storage::path_is_within(path, parent))
        || path.components().any(|part| part.as_os_str() == ".opendam")
        || (path.is_dir() && path.join(".lumiadam-catalog").exists())
}

async fn source_roots(db: &Database, library: &LibraryHandle) -> Result<Vec<PathBuf>> {
    let json: Option<String> = sqlx::query_scalar("SELECT root_paths FROM libraries WHERE id = ?")
        .bind(&library.id).fetch_optional(&db.master_pool).await?;
    Ok(match json {
        Some(json) => serde_json::from_str(&json)?,
        None => vec![library.root_path.clone()],
    })
}

// Boş kapsam yalnız birincil köke göre göreli yolları kapsar; ikinci kökün mutlak yollarını silmez.
fn scoped_select(table: &str, columns: &str) -> String {
    let collation = if cfg!(windows) { "NOCASE" } else { "BINARY" };
    let column = if table == "assets" { "file_path" } else { "path" };
    let value = format!("replace({column}, char(92), '/')");
    format!("SELECT {columns} FROM {table} WHERE (? = '' AND {value} NOT GLOB '/*' AND {value} NOT GLOB '[A-Za-z]:/*') OR (? <> '' AND ({value} = ? COLLATE {collation} OR ({value} >= ? COLLATE {collation} AND {value} < ? COLLATE {collation})))")
}

/// Kaynak dosyalarını değiştirmez; yalnız başarılı dolaşımdan sonra kapsanan eski kayıtları siler.
/// Manuel tam Rescan aynı girişten bütün kayıtlı kaynak köklerini geçirir.
pub async fn index_paths(db: &Database, library_id: &str, paths: &[String]) -> Result<()> {
    let result = scan_paths(db, library_id, paths).await;
    if let Err(error) = &result {
        record_error(db, library_id, &error.to_string()).await;
    }
    result.map(|_| ())
}

async fn scan_paths(db: &Database, library_id: &str, paths: &[String]) -> Result<bool> {
    let library = db.get_library_handle(Some(library_id)).await
        .ok_or_else(|| anyhow::anyhow!("Library catalog is not open"))?;
    let _scan = library.scan_lock.lock().await;
    library.scan_processed.store(0, Ordering::Relaxed);
    library.scan_full.store(false, Ordering::Relaxed);
    anyhow::ensure!(!paths.is_empty(), "No source folders to scan");
    let roots = source_roots(db, &library).await?;
    let master_path = db.master_pool.connect_options().get_filename().to_string_lossy().into_owned();
    let mut internal = vec![db.storage_root.clone(), db.default_storage_root.clone(), library.catalog_dir.clone()];
    internal.extend(["", "-journal", "-wal", "-shm"].iter().map(|suffix| PathBuf::from(format!("{master_path}{suffix}"))));
    let requested: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let validated = tokio::task::spawn_blocking({
        let roots = roots.clone();
        let internal = internal.clone();
        move || -> Result<Vec<PathBuf>> {
            let mut scopes: Vec<PathBuf> = Vec::new();
            for path in requested {
                anyhow::ensure!(path.is_absolute() && !path.components().any(|p| matches!(p, Component::ParentDir)), "Scan path must be an absolute source folder");
                let root = roots.iter().find(|root| crate::storage::path_is_within(&path, root))
                    .ok_or_else(|| anyhow::anyhow!("Scan path is outside the registered source folders"))?;
                anyhow::ensure!(!excluded(&path, &internal), "Internal catalog paths cannot be scanned");
                anyhow::ensure!(root.is_dir() && path.is_dir(), "Source is unavailable; the catalog is preserved: {}", path.display());
                std::fs::read_dir(root)?;
                let real_root = std::fs::canonicalize(root)?;
                let real_path = std::fs::canonicalize(&path)?;
                anyhow::ensure!(crate::storage::path_is_within(&real_path, &real_root) && !excluded(&real_path, &internal), "Scan path resolves outside the source folder");
                if scopes.iter().any(|scope| crate::storage::path_is_within(&path, scope)) { continue; }
                scopes.retain(|scope| !crate::storage::path_is_within(scope, &path));
                scopes.push(path);
            }
            Ok(scopes)
        }
    }).await??;
    let full_scan = roots.iter().all(|root| validated.iter().any(|scope| crate::storage::path_is_within(root, scope)));
    library.scan_full.store(full_scan, Ordering::Relaxed);
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(format!("scan_error:{library_id}")).execute(&db.master_pool).await?;
    let now = Utc::now().timestamp_millis();
    let scopes: Vec<String> = validated.iter().map(|path| make_relative_path(&library.root_path, path)).collect();
    let mut existing: HashMap<String, (String, String, i64, i64)> = HashMap::new();
    let mut old_folders: HashMap<String, String> = HashMap::new();
    for scope in &scopes {
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(&scoped_select("assets", "id,file_path,modified_at,size_bytes"))
            .bind(scope).bind(scope).bind(scope).bind(format!("{scope}/")).bind(format!("{scope}0"))
            .fetch_all(&library.pool).await?;
        for (id, path, modified, size) in rows { existing.insert(path_key(&path), (id, path, modified, size)); }
        let rows: Vec<String> = sqlx::query_scalar(&scoped_select("folders", "path"))
            .bind(scope).bind(scope).bind(scope).bind(format!("{scope}/")).bind(format!("{scope}0"))
            .fetch_all(&library.pool).await?;
        for path in rows { old_folders.insert(path_key(&path), path); }
    }

    // NAS dolaşımı ve metadata okumaları sınırlı kanaldan akar; Tokio işçi havuzu bloklanmaz.
    let (sender, mut receiver) = mpsc::channel::<ScannedEntry>(256);
    let walk = tokio::task::spawn_blocking({
        let scan_scopes = validated.clone();
        move || -> Result<()> {
            let real_roots: Vec<PathBuf> = roots.iter().filter_map(|root| std::fs::canonicalize(root).ok()).collect();
            for root in scan_scopes {
                let walker = WalkDir::new(&root).follow_links(true).into_iter()
                    .filter_entry(|entry| !excluded(entry.path(), &internal));
                for entry in walker {
                    let entry = entry?;
                    if entry.path_is_symlink() {
                        let target = std::fs::canonicalize(entry.path())?;
                        anyhow::ensure!(real_roots.iter().any(|root| crate::storage::path_is_within(&target, root)) && !excluded(&target, &internal), "Source link resolves outside the registered folders");
                    }
                    if !entry.file_type().is_dir() && !entry.file_type().is_file() { continue; }
                    let file = if entry.file_type().is_file() {
                        let metadata = entry.metadata()?;
                        let modified = metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_millis() as i64;
                        Some((metadata.len() as i64, modified))
                    } else { None };
                    if sender.blocking_send(ScannedEntry { path: entry.into_path(), file }).is_err() { return Ok(()); }
                }
                // Bağlantı dolaşımın sonunda kesilirse silme aşamasına geçme.
                std::fs::read_dir(&root)?;
            }
            Ok(())
        }
    });
    let mut batch = Vec::with_capacity(200);
    let mut new_folders = HashSet::new();
    let mut changed = false;
    while let Some(entry) = receiver.recv().await {
        let relative = make_relative_path(&library.root_path, &entry.path);
        let key = path_key(&relative);
        let Some((size_bytes, modified_at)) = entry.file else {
            if !relative.is_empty() && old_folders.remove(&key).is_none() { new_folders.insert(relative); }
            continue;
        };
        library.scan_processed.fetch_add(1, Ordering::Relaxed);
        let cached = existing.remove(&key);
        if cached.as_ref().is_some_and(|(_, old_path, modified, size)| *modified == modified_at && *size == size_bytes && old_path == &relative) { continue; }
        let extension = entry.path.extension().and_then(|value| value.to_str()).unwrap_or("").to_lowercase();
        let file_name = entry.path.file_name().and_then(|value| value.to_str()).unwrap_or("").to_string();
        let needs_job = crate::preview::is_previewable(&extension);
        let is_update = cached.is_some();
        let id = cached.map(|(id, _, _, _)| id).unwrap_or_else(|| Uuid::new_v4().to_string());
        batch.push(PendingAsset {
            id, file_path: relative, file_name, kind_str: AssetKind::from_extension(&extension).as_str(), extension,
            size_bytes, modified_at, now, preview_status: if needs_job { "pending" } else { "none" }, needs_job, is_update,
        });
        if batch.len() >= 200 {
            commit_asset_batch(&library.pool, &batch).await?;
            changed = true;
            db.preview_wakeup.notify_waiters();
            batch.clear();
        }
    }
    // Herhangi bir WalkDir/metadata hatası eski dosya ve klasörlerin silinmesini engeller.
    walk.await??;
    if !batch.is_empty() { commit_asset_batch(&library.pool, &batch).await?; changed = true; db.preview_wakeup.notify_waiters(); }

    if !existing.is_empty() || !old_folders.is_empty() || !new_folders.is_empty() {
        let mut tx = library.pool.begin().await?;
        for (id, _, _, _) in existing.values() {
            // Eski kataloglarda foreign_keys kapalı olabildiği için bağlı satırları açıkça temizle.
            for table in ["preview_jobs", "asset_tags", "collection_assets"] {
                sqlx::query(&format!("DELETE FROM {table} WHERE asset_id = ?")).bind(id).execute(&mut *tx).await?;
            }
            sqlx::query("DELETE FROM assets WHERE id = ?").bind(id).execute(&mut *tx).await?;
        }
        for path in old_folders.values() {
            sqlx::query("DELETE FROM folders WHERE path = ?").bind(path).execute(&mut *tx).await?;
        }
        for path in new_folders {
            let (parent, name) = path.rsplit_once('/').unwrap_or(("", &path));
            sqlx::query("INSERT OR IGNORE INTO folders(path,name,parent) VALUES(?,?,?)")
                .bind(&path).bind(name).bind(parent).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        changed = true;
    }
    if changed {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets").fetch_one(&library.pool).await?;
        sqlx::query("UPDATE libraries SET asset_count=? WHERE id=?").bind(count).bind(library_id).execute(&db.master_pool).await?;
    }
    if full_scan {
        sqlx::query("INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
            .bind(format!("last_scan:{library_id}")).bind(Utc::now().timestamp_millis().to_string()).execute(&db.master_pool).await?;
    }
    tracing::info!(library_id, scopes = scopes.len(), scanned_files = library.scan_processed.load(Ordering::Relaxed), changed, "Library scan complete");
    Ok(changed)
}

/// Değişen kaynağın önizlemesini ve eski iş kimliklerini aynı transaction içinde geçersizleştirir.
async fn commit_asset_batch(pool: &sqlx::Pool<sqlx::Sqlite>, batch: &[PendingAsset]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for item in batch {
        if item.is_update {
            sqlx::query("UPDATE assets SET file_path=?,file_name=?,extension=?,kind=?,size_bytes=?,modified_at=?,preview_status=?,thumbnail_path=NULL,metadata=json_remove(coalesce(nullif(metadata,''),'{}'),'$.preview_model') WHERE id=?")
                .bind(&item.file_path).bind(&item.file_name).bind(&item.extension).bind(item.kind_str)
                .bind(item.size_bytes).bind(item.modified_at).bind(item.preview_status).bind(&item.id).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM preview_jobs WHERE asset_id=?").bind(&item.id).execute(&mut *tx).await?;
        } else {
            sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at,rating,color_label,is_favorite,description,thumbnail_path,preview_status,metadata) VALUES(?,?,?,?,?,?,?,?,0,'none',0,'',NULL,?,'{}')")
                .bind(&item.id).bind(&item.file_path).bind(&item.file_name).bind(&item.extension).bind(item.kind_str)
                .bind(item.size_bytes).bind(item.modified_at).bind(item.now).bind(item.preview_status).execute(&mut *tx).await?;
        }
        if item.needs_job {
            sqlx::query("INSERT INTO preview_jobs(id,asset_id,status,created_at) VALUES(?,?,'pending',?)")
                .bind(Uuid::new_v4().to_string()).bind(&item.id).bind(item.now).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

async fn record_error(db: &Database, id: &str, error: &str) {
    tracing::warn!(library_id = id, error, "Library update failed; use manual Rescan if notifications are unavailable");
    let _ = sqlx::query("INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
        .bind(format!("scan_error:{id}")).bind(error).execute(&db.master_pool).await;
}

struct WatchRegistration {
    roots: Vec<PathBuf>,
    _watcher: Option<RecommendedWatcher>,
    overflow: Arc<AtomicBool>,
    retry_at: Instant,
}

#[derive(Default)]
pub struct LibraryWatcher {
    registrations: HashMap<String, WatchRegistration>,
}
impl LibraryWatcher {
    pub fn new() -> Self { Self::default() }
}

struct WatchMessage {
    library_id: String,
    event: notify::Result<Event>,
}
struct ScanRequest {
    library_id: String,
    paths: Vec<String>,
}
#[derive(Default)]
struct PendingScan {
    full: bool,
    paths: HashSet<PathBuf>,
    last_event: Option<Instant>,
}

fn coalesce_path(pending: &mut PendingScan, path: PathBuf) {
    pending.last_event = Some(Instant::now());
    if pending.full || pending.paths.iter().any(|parent| crate::storage::path_is_within(&path, parent)) { return; }
    pending.paths.retain(|child| !crate::storage::path_is_within(child, &path));
    pending.paths.insert(path);
    if pending.paths.len() > 64 { pending.paths.clear(); pending.full = true; }
}

fn event_scopes(event: &Event, roots: &[PathBuf], internal: &[PathBuf]) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for path in &event.paths {
        if path.components().any(|part| matches!(part, Component::ParentDir)) || excluded(path, internal) { continue; }
        let Some(root) = roots.iter().find(|root| crate::storage::path_is_within(path, root)) else { continue; };
        let mut directory = path.as_path();
        while !directory.is_dir() {
            let Some(parent) = directory.parent() else { break; };
            if !crate::storage::path_is_within(parent, root) { break; }
            directory = parent;
        }
        if directory.is_dir() && crate::storage::path_is_within(directory, root) { result.push(directory.to_path_buf()); }
    }
    result
}

/// İşletim sisteminin desteklediği bildirimleri toplar. Kaçırılan NAS olayları için manuel Rescan korunur.
pub fn start_library_watchers(state: Arc<AppState>) {
    let (events_tx, mut events_rx) = mpsc::channel::<WatchMessage>(256);
    let (scans_tx, mut scans_rx) = mpsc::channel::<ScanRequest>(8);
    tauri::async_runtime::spawn({
        let state = state.clone();
        async move {
            while let Some(request) = scans_rx.recv().await {
                if state.db.get_library_handle(Some(&request.library_id)).await.is_none() { continue; }
                let _ = state.app_handle.emit("library:activity", serde_json::json!({"libraryId":request.library_id,"scanning":true}));
                let result = scan_paths(&state.db, &request.library_id, &request.paths).await;
                if let Err(error) = &result { record_error(&state.db, &request.library_id, &error.to_string()).await; }
                if !matches!(result, Ok(false)) { let _ = state.app_handle.emit("library:updated", ()); }
                let _ = state.app_handle.emit("library:activity", serde_json::json!({"libraryId":request.library_id,"scanning":false}));
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_millis(250));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut topology_at = Instant::now();
        let mut pending: HashMap<String, PendingScan> = HashMap::new();
        loop {
            tokio::select! {
                Some(message) = events_rx.recv() => {
                    let (roots, internal) = {
                        let watcher = state.watcher.lock().await;
                        let Some(registration) = watcher.registrations.get(&message.library_id) else { continue; };
                        (registration.roots.clone(), vec![state.db.storage_root.clone(), state.db.default_storage_root.clone()])
                    };
                    let item = pending.entry(message.library_id.clone()).or_default();
                    match message.event {
                        Ok(event) if matches!(event.kind, EventKind::Access(_)) => {},
                        Ok(event) if !event.need_rescan() && event.paths.len() <= 256 => {
                            let scopes = tokio::task::spawn_blocking(move || event_scopes(&event, &roots, &internal)).await;
                            if let Ok(scopes) = scopes { for path in scopes { coalesce_path(item, path); } }
                            else { item.full = true; item.last_event = Some(Instant::now()); }
                        },
                        _ => { item.full = true; item.paths.clear(); item.last_event = Some(Instant::now()); }
                    }
                },
                _ = timer.tick() => {
                    if Instant::now() >= topology_at {
                        topology_at = Instant::now() + Duration::from_secs(3);
                        let libraries = state.db.get_all_libraries().await;
                        let ids: HashSet<_> = libraries.iter().map(|library| library.id.clone()).collect();
                        state.watcher.lock().await.registrations.retain(|id, _| ids.contains(id));
                        pending.retain(|id, _| ids.contains(id));
                        for library in libraries {
                            let roots = match source_roots(&state.db, &library).await {
                                Ok(roots) => roots,
                                Err(error) => { record_error(&state.db, &library.id, &error.to_string()).await; continue; }
                            };
                            let needs_registration = state.watcher.lock().await.registrations.get(&library.id)
                                .is_none_or(|current| current.roots != roots || (current._watcher.is_none() && Instant::now() >= current.retry_at));
                            if !needs_registration { continue; }
                            let overflow = Arc::new(AtomicBool::new(false));
                            let build = tokio::task::spawn_blocking({
                                let roots = roots.clone();
                                let overflow = overflow.clone();
                                let sender = events_tx.clone();
                                let id = library.id.clone();
                                move || -> Result<RecommendedWatcher> {
                                    let mut watcher = notify::recommended_watcher(move |event| {
                                        if sender.try_send(WatchMessage { library_id: id.clone(), event }).is_err() {
                                            overflow.store(true, Ordering::Relaxed);
                                        }
                                    })?;
                                    for root in roots { watcher.watch(&root, RecursiveMode::Recursive)?; }
                                    Ok(watcher)
                                }
                            }).await;
                            let native = match build {
                                Ok(Ok(watcher)) => Some(watcher),
                                result => {
                                    record_error(&state.db, &library.id, &format!("Automatic file notifications are unavailable; use manual Rescan: {result:?}")).await;
                                    None
                                }
                            };
                            state.watcher.lock().await.registrations.insert(library.id.clone(), WatchRegistration {
                                roots, _watcher: native, overflow, retry_at: Instant::now() + Duration::from_secs(30),
                            });
                        }
                    }
                    let ready: Vec<_> = {
                        let watcher = state.watcher.lock().await;
                        for (id, registration) in &watcher.registrations {
                            if registration.overflow.swap(false, Ordering::Relaxed) {
                                let item = pending.entry(id.clone()).or_default();
                                item.full = true; item.paths.clear(); item.last_event = Some(Instant::now());
                            }
                        }
                        pending.iter().filter_map(|(id, item)| {
                            if item.last_event.is_none_or(|at| at.elapsed() < Duration::from_millis(750)) { return None; }
                            let registration = watcher.registrations.get(id)?;
                            let paths: Vec<String> = if item.full { registration.roots.iter().map(|path| path.to_string_lossy().into_owned()).collect() }
                                else { item.paths.iter().map(|path| path.to_string_lossy().into_owned()).collect() };
                            (!paths.is_empty()).then_some((id.clone(), paths))
                        }).collect()
                    };
                    for (id, paths) in ready {
                        if scans_tx.try_send(ScanRequest { library_id: id.clone(), paths }).is_ok() { pending.remove(&id); }
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests;
