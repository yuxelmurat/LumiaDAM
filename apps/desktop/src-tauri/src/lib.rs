pub mod commands;
pub mod db;
pub mod models;
pub mod watcher;
pub mod preview;
pub mod search_index;
pub mod storage;
pub mod folders;
pub mod workflow;
pub mod duplicates;
pub mod local_ai;
pub mod preferences;
#[cfg(windows)]
pub mod windows_thumbnail;

use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

use db::Database;
use watcher::LibraryWatcher;

pub struct AppState {
    pub db: Database,
    pub watcher: Mutex<LibraryWatcher>,
    pub app_handle: tauri::AppHandle,
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
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
            let local_dir = if let Some(path) = std::env::var_os("LUMIADAM_TEST_DATA_DIR") {
                // Açılış kontrolünde gerçek kullanıcı kataloğuna dokunmadan aynı uygulamayı çalıştır.
                let path=std::path::PathBuf::from(path);
                if !path.is_absolute() || !path.join("data/opendam.sqlite").is_file() {return Err("Diagnostic catalog must be an existing absolute path".into());}
                path
            } else {dirs::data_local_dir().ok_or("Local application data folder was not found")?.join("LumiaDAM")};
            let data_dir = local_dir.join("data");
            tracing::info!(path = %data_dir.display(), "Opening local catalog");
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
                Database::new(db_path.to_str().ok_or_else(|| anyhow::anyhow!("Invalid data path"))?, local_dir.join("libraries")).await
            })?;

            let watcher = LibraryWatcher::new();
            let state = Arc::new(AppState {
                db,
                watcher: Mutex::new(watcher),
                app_handle: app.handle().clone(),
            });

            app.manage(state.clone());

            let turkish = crate::preferences::get_app_language().is_ok_and(|language| language == "tr");
            let open = MenuItem::with_id(app, "open", if turkish { "LumiaDAM'ı aç" } else { "Open LumiaDAM" }, true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "exit", if turkish { "Çıkış" } else { "Exit" }, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &exit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("LumiaDAM window icon is configured").clone())
                .tooltip("LumiaDAM")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "exit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            crate::local_ai::resume_configured_schedule(state.clone());
            
            // Start the background preview worker queue
            crate::preview::start_preview_worker(state.clone());
            crate::watcher::start_library_watchers(state.clone());

            // Açılışta NAS taraması yok: katalog hemen açılır, kaynak Yenile ile taranır.

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            preferences::get_app_language,
            preferences::set_app_language,
            preferences::acknowledge_close_to_tray,
            local_ai::get_local_ai_settings,
            local_ai::save_local_ai_settings,
            local_ai::get_local_ai_status,
            local_ai::get_local_ai_log,
            local_ai::open_local_ai_log,
            local_ai::start_local_ai,
            local_ai::stop_local_ai,
            local_ai::prepare_local_ai_model,
            local_ai::list_ai_suggestions,
            local_ai::apply_ai_suggestion,
            local_ai::apply_all_ai_suggestions,
            local_ai::dismiss_ai_suggestion,
            commands::max_bridge::get_max_instances,
            commands::max_bridge::merge_to_max,
            commands::max_bridge::get_max_merge_status,
            commands::editor::read_editor_asset,
            commands::editor::save_editor_export,
            commands::library::get_storage_settings,
            commands::library::set_storage_location,
            commands::library::add_library,
            commands::library::list_libraries,
            commands::library::get_library_work_status,
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
            commands::preview::prioritize_previews,
            commands::preview::get_preview_jobs,
            commands::shell::reveal_in_explorer,
            commands::shell::open_with_default,
            commands::workflow::get_path_facets,
            commands::workflow::set_path_labels,
            commands::workflow::set_related_image,
            commands::workflow::list_related_images,
            commands::workflow::resolve_asset_file,
            commands::workflow::keep_offline,
            commands::workflow::remove_offline,
            commands::workflow::get_project_notes,
            commands::workflow::save_project_note,
            commands::workflow::update_project,
            commands::workflow::check_asset_availability,
            commands::workflow::find_duplicates,
            commands::workflow::cancel_duplicate_scan,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if crate::preferences::close_to_tray_acknowledged() {
                    let _ = window.hide();
                } else {
                    let _ = window.emit("app:close-to-tray-reminder", ());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
