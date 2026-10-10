pub mod commands;
pub mod db;
pub mod models;
pub mod watcher;
pub mod preview;
pub mod storage;

use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

use db::Database;
use watcher::LibraryWatcher;

pub struct AppState {
    pub db: Database,
    pub watcher: Mutex<LibraryWatcher>,
    pub app_handle: tauri::AppHandle,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "lumiadam_desktop_lib=debug,info".parse().unwrap()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let legacy_master = app.path().app_data_dir()?.join("opendam.sqlite");
            let local_dir = dirs::data_local_dir()
                .ok_or("Yerel uygulama verisi klasörü bulunamadı")?.join("LumiaDAM");
            let data_dir = local_dir.join("data");
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("opendam.sqlite");
            let rt = tokio::runtime::Runtime::new()?;
            let db = rt.block_on(async {
                if !db_path.try_exists()? && legacy_master.try_exists()? {
                    let temporary = data_dir.join(format!("migration-{}.sqlite", uuid::Uuid::new_v4()));
                    let result = storage::snapshot(&legacy_master, &temporary).await;
                    if let Err(error) = result {
                        let _ = std::fs::remove_file(&temporary);
                        return Err(error);
                    }
                    std::fs::rename(temporary, &db_path)?;
                }
                Database::new(db_path.to_str().ok_or_else(|| anyhow::anyhow!("Geçersiz veri yolu"))?, local_dir.join("libraries")).await
            })?;

            let watcher = LibraryWatcher::new();
            let state = Arc::new(AppState {
                db,
                watcher: Mutex::new(watcher),
                app_handle: app.handle().clone(),
            });

            app.manage(state.clone());
            
            // Start the background preview worker queue
            crate::preview::start_preview_worker(state.clone());

            // Açılışta NAS taraması yok: katalog hemen açılır, kaynak Yenile ile taranır.

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::get_storage_settings,
            commands::library::set_storage_location,
            commands::library::add_library,
            commands::library::list_libraries,
            commands::library::remove_library,
            commands::library::refresh_library,
            commands::library::refresh_all_libraries,
            commands::asset::search_assets,
            commands::asset::get_asset,
            commands::asset::update_asset,
            commands::asset::delete_asset,
            commands::asset::delete_assets,
            commands::asset::rename_asset,
            commands::tag::list_tags,
            commands::tag::create_tag,
            commands::tag::delete_tag,
            commands::collection::list_collections,
            commands::collection::create_collection,
            commands::collection::add_to_collection,
            commands::collection::remove_from_collection,
            commands::collection::delete_collection,
            commands::preview::request_preview,
            commands::preview::get_preview_jobs,
            commands::shell::reveal_in_explorer,
            commands::shell::open_with_default,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
