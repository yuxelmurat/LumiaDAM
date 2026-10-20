use std::sync::Arc;
use std::path::PathBuf;
use tauri::{State, Emitter};
use uuid::Uuid;
use chrono::Utc;
use serde_json;
use std::sync::atomic::Ordering;

use crate::AppState;
use crate::models::{Library, AddLibraryPayload};
use crate::watcher;

type Result<T> = std::result::Result<T, String>;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryWorkStatus {
    id: String,
    name: String,
    scanning: bool,
    scanned_files: u64,
    estimated_remaining: Option<u64>,
    last_scan_at: Option<i64>,
    last_error: Option<String>,
    preview_total: i64,
    preview_completed: i64,
    preview_pending: i64,
    preview_failed: i64,
    preview_paused: bool,
}

#[tauri::command]
pub async fn get_library_work_status(state: State<'_, Arc<AppState>>) -> Result<Vec<LibraryWorkStatus>> {
    let started = std::time::Instant::now();
    let libraries = state.db.get_all_libraries().await;
    let mut statuses = Vec::with_capacity(libraries.len());
    for library in libraries {
        let scanning = library.scan_lock.try_lock().is_err();
        let scanned_files = library.scan_processed.load(Ordering::Relaxed);
        let (previous_count, last_scan, last_error): (i64, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT asset_count, (SELECT value FROM settings WHERE key = ?), (SELECT value FROM settings WHERE key = ?) FROM libraries WHERE id = ?"
        ).bind(format!("last_scan:{}", library.id)).bind(format!("scan_error:{}", library.id)).bind(&library.id)
            .fetch_optional(&state.db.master_pool).await.map_err(|e| e.to_string())?.unwrap_or((0,None,None));
        let last_scan_at = last_scan.and_then(|value| value.parse::<i64>().ok());
        let (preview_total, preview_completed, preview_pending, preview_failed): (i64, i64, i64, i64) =
            sqlx::query_as("SELECT total, completed, pending, failed FROM preview_summary")
                .fetch_one(&library.pool).await.map_err(|e| e.to_string())?;
        let generating: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preview_jobs WHERE status='processing')").fetch_one(&library.pool).await.map_err(|e|e.to_string())?;
        statuses.push(LibraryWorkStatus {
            id: library.id,
            name: library.name,
            scanning,
            scanned_files,
            estimated_remaining: (scanning && library.scan_full.load(Ordering::Relaxed) && previous_count > 0)
                .then_some((previous_count as u64).saturating_sub(scanned_files)),
            last_scan_at,
            last_error,
            preview_total,
            preview_completed,
            preview_pending,
            preview_failed,
            preview_paused: preview_pending > 0 && !generating && library.preview_retry_at.load(Ordering::Relaxed) > 0,
        });
    }
    tracing::debug!(elapsed_ms = started.elapsed().as_millis(), libraries = statuses.len(), "Library activity snapshot");
    Ok(statuses)
}

async fn record_scan_error(db: &crate::db::Database, id: &str, error: &str) {
    let _ = sqlx::query("INSERT INTO settings(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(format!("scan_error:{id}"))
        .bind(error)
        .execute(&db.master_pool).await;
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSettings {
    active_path: String,
    selected_path: String,
    default_path: String,
    restart_required: bool,
    errors: Vec<String>,
}

#[tauri::command]
pub async fn get_storage_settings(state: State<'_, Arc<AppState>>) -> Result<StorageSettings> {
    let selected: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'storage_root'")
        .fetch_optional(&state.db.master_pool).await.map_err(|e| e.to_string())?;
    let default_path = state.db.default_storage_root.to_string_lossy().to_string();
    let selected_path = selected.unwrap_or_else(|| default_path.clone());
    let active_path = state.db.storage_root.to_string_lossy().to_string();
    Ok(StorageSettings {
        restart_required: selected_path != active_path,
        active_path, selected_path, default_path, errors: state.db.storage_errors.clone(),
    })
}

#[tauri::command]
pub async fn set_storage_location(path: String, state: State<'_, Arc<AppState>>) -> Result<StorageSettings> {
    let location = PathBuf::from(&path);
    let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT root_paths, catalog_path FROM libraries")
        .fetch_all(&state.db.master_pool).await.map_err(|e| e.to_string())?;
    let mut roots = Vec::new();
    let mut catalogs = Vec::new();
    for (paths, catalog) in rows {
        let paths: Vec<PathBuf> = serde_json::from_str(&paths).map_err(|e| e.to_string())?;
        roots.extend(paths);
        if let Some(catalog) = catalog { catalogs.push(PathBuf::from(catalog)); }
    }
    // Sürücünün tamamı kaynak olsa da varsayılan yerel veri alanı kullanılabilir.
    if location == state.db.default_storage_root {
        std::fs::create_dir_all(&location).map_err(|e| e.to_string())?;
        roots.clear();
    }
    crate::storage::validate_location(&location, &roots, &catalogs).map_err(|e| format!("{:#}", e))?;
    sqlx::query("INSERT INTO settings(key, value) VALUES('storage_root', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(path).execute(&state.db.master_pool).await.map_err(|e| e.to_string())?;
    get_storage_settings(state).await
}

#[tauri::command]
pub async fn add_library(
    payload: AddLibraryPayload,
    state: State<'_, Arc<AppState>>,
) -> Result<Library> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp_millis();
    let root_paths_json = serde_json::to_string(&payload.root_paths).map_err(|e| e.to_string())?;

    let first_path = payload.root_paths.first().ok_or("No root path provided")?;
    let root_path = PathBuf::from(first_path);
    let name = crate::db::library_name(&payload.name, first_path);

    // Katalog arşivden ayrı depolanır; eski katalog varsa güvenli kopyası alınır.
    let handle = state.db.register_library(&id, &name, &root_path)
        .await
        .map_err(|e| e.to_string())?;

    // Record in master database
    sqlx::query(
        "INSERT INTO libraries (id, name, root_paths, created_at, asset_count, catalog_path) VALUES (?, ?, ?, ?, 0, ?)"
    )
    .bind(&id)
    .bind(&name)
    .bind(&root_paths_json)
    .bind(now)
    .bind(handle.catalog_dir.to_string_lossy().as_ref())
    .execute(&state.db.master_pool)
    .await
    .map_err(|e| e.to_string())?;

    // Kick off background NAS-efficient indexing
    let state_clone = Arc::clone(&state);
    let paths = payload.root_paths.clone();
    let lib_id = id.clone();
    tokio::spawn(async move {
        let _ = state_clone.app_handle.emit("library:activity", ());
        if let Err(e) = watcher::index_paths(&state_clone.db, &lib_id, &paths).await {
            tracing::error!("Failed to index library {}: {}", lib_id, e);
            record_scan_error(&state_clone.db, &lib_id, &e.to_string()).await;
        }
        // Tarama yarıda kalsa da içe alınan katalog ve tamamlanan kayıtlar görünür olmalı.
        let _ = state_clone.app_handle.emit("library:updated", ());
    });

    Ok(Library {
        id,
        name,
        root_paths: payload.root_paths,
        created_at: now,
        asset_count: 0,
    })
}

#[tauri::command]
pub async fn list_libraries(state: State<'_, Arc<AppState>>) -> Result<Vec<Library>> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
        "SELECT id, name, root_paths, created_at, asset_count FROM libraries ORDER BY created_at ASC"
    )
    .fetch_all(&state.db.master_pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.into_iter()
        .map(|(id, name, root_paths_json, created_at, asset_count)| {
            let root_paths: Vec<String> =
                serde_json::from_str(&root_paths_json).unwrap_or_default();
            Ok(Library { id, name, root_paths, created_at, asset_count })
        })
        .collect()
}

#[tauri::command]
pub async fn remove_library(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let _ = state.db.unregister_library(&id).await;

    sqlx::query("DELETE FROM libraries WHERE id = ?")
        .bind(&id)
        .execute(&state.db.master_pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn refresh_library(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT root_paths FROM libraries WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.db.master_pool)
    .await
    .map_err(|e| e.to_string())?;

    if let Some((root_paths_json,)) = row {
        let paths: Vec<String> = serde_json::from_str(&root_paths_json).unwrap_or_default();
        let _ = state.app_handle.emit("library:activity", ());
        if let Err(error) = watcher::index_paths(&state.db, &id, &paths).await {
            record_scan_error(&state.db, &id, &error.to_string()).await;
            let _ = state.app_handle.emit("library:activity", ());
            return Err(error.to_string());
        }
        let _ = state.app_handle.emit("library:updated", ());
    }
    Ok(())
}

#[tauri::command]
pub async fn refresh_all_libraries(
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, root_paths FROM libraries"
    )
    .fetch_all(&state.db.master_pool)
    .await
    .map_err(|e| e.to_string())?;

    for (id, root_paths_json) in rows {
        let paths: Vec<String> = serde_json::from_str(&root_paths_json).unwrap_or_default();
        let _ = state.app_handle.emit("library:activity", ());
        if let Err(error) = watcher::index_paths(&state.db, &id, &paths).await {
            record_scan_error(&state.db, &id, &error.to_string()).await;
            let _ = state.app_handle.emit("library:activity", ());
            return Err(error.to_string());
        }
    }

    let _ = state.app_handle.emit("library:updated", ());
    Ok(())
}
