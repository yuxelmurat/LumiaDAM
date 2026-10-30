//! İsteğe bağlı yerel analiz; kapalı modda model, servis ve zamanlayıcı başlatılmaz.
use crate::{db::resolve_path, AppState};
use anyhow::{ensure, Context, Result};
use chrono::{Local, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tauri::async_runtime::JoinHandle;
use tauri::{Emitter, State};
mod native;
mod runtime;
mod activity;

const CATEGORIES: &[&str] = &[
    "Kitchen",
    "Living room",
    "Bedroom",
    "Bathroom",
    "Office",
    "Dining room",
    "Dressing room",
    "Exterior",
    "Furniture & Decor",
    "Texture",
    "Other",
];

fn texture_allowed_for(extension: &str) -> bool {
    matches!(extension.trim_start_matches('.').to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "tif" | "tiff" | "tga" | "bmp" | "webp" | "exr" | "hdr")
}

async fn remove_stale_texture_membership(pool: &sqlx::SqlitePool, asset_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM collection_assets WHERE collection_id='local-ai:category:Texture' AND asset_id=?")
        .bind(asset_id).execute(pool).await?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiSettings {
    pub mode: String,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub idle_minutes: u32,
    pub start_hour: u32,
    pub end_hour: u32,
    pub cpu_only: bool,
    pub batch_size: u32,
    pub setup_complete: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            mode: "disabled".into(),
            provider: "bundled".into(),
            endpoint: "http://127.0.0.1:11434".into(),
            model: "qwen3.5:0.8b".into(),
            idle_minutes: 10,
            start_hour: 1,
            end_hour: 6,
            cpu_only: true,
            batch_size: 100,
            setup_complete: false,
        }
    }
}

fn permits_run(
    settings: &AiSettings,
    manual: bool,
    hour: u32,
    idle_seconds: u64,
    quiet: bool,
) -> bool {
    if !settings.setup_complete || settings.mode == "disabled" {
        return false;
    }
    if manual {
        return true;
    }
    match settings.mode.as_str() {
        "idle" => quiet && idle_seconds >= u64::from(settings.idle_minutes) * 60,
        "scheduled" if settings.start_hour < settings.end_hour => {
            (settings.start_hour..settings.end_hour).contains(&hour)
        }
        "scheduled" if settings.start_hour > settings.end_hour => {
            hour >= settings.start_hour || hour < settings.end_hour
        }
        _ => false,
    }
}

fn local_endpoint(value: &str) -> Result<String> {
    let mut url = reqwest::Url::parse(value)?;
    ensure!(
        url.scheme() == "http"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "Use a local HTTP server address without credentials or a path."
    );
    if url.host_str() == Some("localhost") {
        url.set_host(Some("127.0.0.1"))?;
    }
    ensure!(
        url.host_str()
            .and_then(|host| host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .ok())
            .is_some_and(|ip| ip.is_loopback()),
        "Local AI can only connect to a loopback address on this computer."
    );
    ensure!(
        url.port_or_known_default().is_some_and(|port| port > 0),
        "Invalid local server port."
    );
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn parse_category(value: &str) -> Result<String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Classification {
        category: String,
        confidence: f32,
    }
    ensure!(value.len() < 4096, "Model classification is too large.");
    let output: Classification = serde_json::from_str(value)?;
    ensure!(
        CATEGORIES.contains(&output.category.as_str()),
        "Unknown classification category."
    );
    ensure!(
        output.confidence.is_finite() && (0.0..=1.0).contains(&output.confidence),
        "Invalid classification confidence."
    );
    Ok(if output.confidence >= 0.8 { output.category } else { "Other".into() })
}

fn validate_settings(settings: &mut AiSettings) -> Result<()> {
    ensure!(
        ["disabled", "manual", "idle", "scheduled"].contains(&settings.mode.as_str()),
        "Unknown Local AI mode."
    );
    ensure!(
        ["bundled", "ollama", "lmstudio"].contains(&settings.provider.as_str()),
        "Unknown Local AI provider."
    );
    ensure!(
        (1..=240).contains(&settings.idle_minutes)
            && settings.start_hour < 24
            && settings.end_hour < 24
            && (1..=1000).contains(&settings.batch_size),
        "Local AI limits are outside their allowed range."
    );
    ensure!(
        settings.mode != "scheduled" || settings.start_hour != settings.end_hour,
        "Choose different start and end hours."
    );
    settings.endpoint = local_endpoint(&settings.endpoint)?;
    ensure!(
        !settings.model.trim().is_empty()
            && settings.model.len() <= 200
            && !settings.model.chars().any(char::is_control),
        "Choose a local model identifier."
    );
    if settings.provider != "lmstudio" {
        ensure!(
            settings.model == "qwen3.5:0.8b",
            "Only the bundled local vision model is supported by Ollama."
        );
    }
    Ok(())
}

fn ai_root() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("Local application data is unavailable.")?
        .join("LumiaDAM/local-ai"))
}

fn read_settings_at(root: &Path) -> Result<AiSettings> {
    match std::fs::read(root.join("settings.json")) {
        Ok(bytes) => {
            ensure!(bytes.len() <= 16384, "Local AI settings file is too large.");
            let mut settings: AiSettings = serde_json::from_slice(&bytes)?;
            validate_settings(&mut settings)?;
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AiSettings::default()),
        Err(error) => Err(error.into()),
    }
}

fn save_settings_at(root: &Path, settings: &AiSettings) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let temporary = root.join(format!("settings-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(settings)?)?;
    std::fs::rename(&temporary, root.join("settings.json"))?;
    Ok(())
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiStatus {
    state: String,
    phase: String,
    processed: usize,
    total: usize,
    suggestions: usize,
    error: Option<String>,
    running: bool,
    engine_running: bool,
    model_loaded: bool,
    last_run_at: Option<i64>,
    run_id: Option<String>,
    catalog_assets: usize,
    analyzed_assets: usize,
    pending_assets: usize,
    without_preview: usize,
    skipped: usize,
}

struct Control {
    status: Mutex<AiStatus>,
    cancelled: Arc<AtomicBool>,
    active: AtomicBool,
    job: Mutex<Option<JoinHandle<()>>>,
    scheduler: Mutex<Option<JoinHandle<()>>>,
    lifecycle: tokio::sync::Mutex<()>,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            status: Mutex::new(activity::restore_status().unwrap_or_else(|| AiStatus {
                state: "off".into(),
                phase: "Local AI is stopped. No model is loaded.".into(),
                ..Default::default()
            })),
            cancelled: Arc::new(AtomicBool::new(false)),
            active: AtomicBool::new(false),
            job: Mutex::new(None),
            scheduler: Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
        }
    }
}
static CONTROL: OnceLock<Control> = OnceLock::new();
static SUGGESTION_MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn control() -> &'static Control {
    CONTROL.get_or_init(Control::default)
}
fn update_status(update: impl FnOnce(&mut AiStatus)) {
    let mut status = control().status.lock().unwrap();
    let before = (status.state.clone(), status.phase.clone(), status.running, status.engine_running, status.model_loaded);
    update(&mut status);
    if before != (status.state.clone(), status.phase.clone(), status.running, status.engine_running, status.model_loaded) {
        activity::record("status_changed", serde_json::json!({ "status": &*status }));
    }
}

#[tauri::command]
pub async fn get_local_ai_log() -> std::result::Result<activity::AiLog, String> {
    tokio::task::spawn_blocking(activity::read).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_local_ai_log() -> std::result::Result<(), String> {
    let log = get_local_ai_log().await?;
    crate::commands::shell::open_with_default(log.path).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiOverview {
    settings: AiSettings,
    runtime_installed: bool,
    install_requested: bool,
}

#[tauri::command]
pub fn get_local_ai_settings() -> std::result::Result<AiOverview, String> {
    let root = ai_root().map_err(|e| e.to_string())?;
    Ok(AiOverview {
        settings: read_settings_at(&root).map_err(|e| e.to_string())?,
        runtime_installed: root.join("runtime/ollama.exe").is_file(),
        install_requested: std::fs::read(root.join("install-requested.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| value["requested"] == true),
    })
}

#[tauri::command]
pub fn get_local_ai_status() -> AiStatus {
    control().status.lock().unwrap().clone()
}

async fn stop_work() {
    let ctl = control();
    if let Some(task) = ctl.scheduler.lock().unwrap().take() {
        task.abort();
    }
    ctl.cancelled.store(true, Ordering::SeqCst);
    update_status(|status| {
        if status.running {
            status.state = "stopping".into();
            status.phase = "Stopping and releasing model resources…".into();
        }
    });
    let job = ctl.job.lock().unwrap().take();
    if let Some(mut job) = job {
        if tokio::time::timeout(Duration::from_secs(15), &mut job)
            .await
            .is_err()
        {
            job.abort();
            let _ = job.await;
        }
    }
    ctl.active.store(false, Ordering::SeqCst);
    update_status(|status| {
        status.running = false;
        status.engine_running = false;
        status.model_loaded = false;
        status.state = "off".into();
        status.phase = "Local AI is stopped. No model is loaded by LumiaDAM.".into();
    });
}

#[tauri::command]
pub async fn stop_local_ai() -> std::result::Result<(), String> {
    activity::record("stop_requested", serde_json::json!({}));
    let _guard = control().lifecycle.lock().await;
    stop_work().await;
    // Durdurma, uygulama yeniden açıldığında da otomatik çalışmayı sürdürmez.
    let root = ai_root().map_err(|e| e.to_string())?;
    let mut settings = read_settings_at(&root).map_err(|e| e.to_string())?;
    if settings.mode != "disabled" {
        settings.mode = "manual".into();
        save_settings_at(&root, &settings).map_err(|e| e.to_string())?;
    }
    activity::record("automatic_runs_paused", serde_json::json!({ "mode": settings.mode }));
    Ok(())
}

#[tauri::command]
pub async fn save_local_ai_settings(
    mut settings: AiSettings,
    state: State<'_, Arc<AppState>>,
) -> std::result::Result<AiOverview, String> {
    if let Err(error) = validate_settings(&mut settings) {
        activity::record("settings_rejected", serde_json::json!({ "error": error.to_string() }));
        return Err(error.to_string());
    }
    let _guard = control().lifecycle.lock().await;
    stop_work().await;
    save_settings_at(&ai_root().map_err(|e| e.to_string())?, &settings)
        .map_err(|e| e.to_string())?;
    activity::record("settings_saved", serde_json::json!({ "settings": &settings }));
    configure_schedule(state.inner().clone(), settings);
    get_local_ai_settings()
}

pub fn resume_configured_schedule(state: Arc<AppState>) {
    // Eski öneri kayıtları hafif bir katalog sorgusuyla temizlenir; model veya analiz başlamaz.
    let cleanup_state = state.clone();
    tauri::async_runtime::spawn(async move {
        match retire_obsolete_similarity_data(&cleanup_state.db).await {
            Ok(removed) => {
                if let Err(error) = refresh_suggestion_count(&cleanup_state.db.master_pool).await {
                    activity::record("obsolete_similarity_cleanup_failed", serde_json::json!({ "error": error.to_string() }));
                } else if removed > 0 {
                    let _ = cleanup_state.app_handle.emit("library:updated", ());
                }
            }
            Err(error) => activity::record("obsolete_similarity_cleanup_failed", serde_json::json!({ "error": error.to_string() })),
        }
    });
    if let Ok(settings) = ai_root().and_then(|root| read_settings_at(&root)) {
        configure_schedule(state, settings);
    }
}

fn configure_schedule(state: Arc<AppState>, settings: AiSettings) {
    if !settings.setup_complete || !["idle", "scheduled"].contains(&settings.mode.as_str()) {
        return;
    }
    update_status(|status| {
        status.state = settings.mode.clone();
        status.phase = "Waiting for your chosen working conditions. No model is loaded.".into();
    });
    let handle = tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if control().active.load(Ordering::SeqCst) {
                continue;
            }
            let idle = runtime::idle_seconds();
            let mut allowed = permits_run(&settings, false, Local::now().hour(), idle, true);
            if allowed && settings.mode == "idle" {
                allowed = tokio::task::spawn_blocking(runtime::machine_quiet)
                    .await
                    .unwrap_or(false);
            }
            if allowed {
                let _guard = control().lifecycle.lock().await;
                let _ = launch(state.clone(), settings.clone(), false, false);
            }
        }
    });
    *control().scheduler.lock().unwrap() = Some(handle);
}

#[tauri::command]
pub async fn start_local_ai(state: State<'_, Arc<AppState>>) -> std::result::Result<(), String> {
    let _guard = control().lifecycle.lock().await;
    let settings =
        read_settings_at(&ai_root().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if !permits_run(&settings, true, Local::now().hour(), 0, false) {
        activity::record("run_rejected", serde_json::json!({ "reason": "Choose and save a working mode first." }));
        return Err("Choose when Local AI may run and save your settings first.".into());
    }
    launch(state.inner().clone(), settings, true, false).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn prepare_local_ai_model(
    state: State<'_, Arc<AppState>>,
) -> std::result::Result<(), String> {
    let _guard = control().lifecycle.lock().await;
    let settings =
        read_settings_at(&ai_root().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if !settings.setup_complete || settings.mode == "disabled" {
        return Err("Choose a working mode and save it before downloading a model.".into());
    }
    if settings.provider == "lmstudio" {
        return Err(
            "Download your local vision model in LM Studio, then select its model identifier here."
                .into(),
        );
    }
    launch(state.inner().clone(), settings, true, true).map_err(|e| e.to_string())
}

fn launch(state: Arc<AppState>, settings: AiSettings, manual: bool, download: bool) -> Result<()> {
    ensure!(
        control()
            .active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok(),
        "A Local AI operation is already running."
    );
    control().cancelled.store(false, Ordering::SeqCst);
    update_status(|status| {
        status.run_id = Some(uuid::Uuid::new_v4().to_string());
        status.state = if download { "downloading" } else { "running" }.into();
        status.running = true;
        status.phase = if download {
            "Preparing model download"
        } else {
            "Finding uncategorized cached previews"
        }
        .into();
        status.error = None;
        status.processed = 0;
        status.total = 0;
        status.skipped = 0;
    });
    activity::record("run_started", serde_json::json!({ "manual": manual, "download": download, "settings": &settings,
        "runId": control().status.lock().unwrap().run_id }));
    let handle = tauri::async_runtime::spawn(async move {
        let result = if download {
            runtime::download(&settings).await
        } else {
            analyze(&state.db, &settings, manual).await
        };
        update_status(|status| {
            status.running = false;
            status.engine_running = false;
            status.model_loaded = false;
            status.last_run_at = Some(chrono::Utc::now().timestamp_millis());
            match result {
                Ok(()) => {
                    status.state = "complete".into();
                    status.phase = if download {
                        "Model downloaded. It is not loaded into memory."
                    } else {
                        "This batch is complete. Review suggestions; more assets may still be waiting."
                    }
                    .into();
                }
                Err(error) => {
                    let cancelled = !should_continue(&settings, manual);
                    status.state = if cancelled { "paused" } else { "error" }.into();
                    status.phase = if cancelled {
                        "Analysis paused. Model resources released.".into()
                    } else {
                        error.to_string()
                    };
                    status.error = if cancelled {
                        None
                    } else {
                        Some(error.to_string())
                    };
                }
            }
        });
        activity::record("run_finished", serde_json::json!({ "status": &*control().status.lock().unwrap() }));
        control().active.store(false, Ordering::SeqCst);
    });
    *control().job.lock().unwrap() = Some(handle);
    Ok(())
}

fn should_continue(settings: &AiSettings, manual: bool) -> bool {
    !control().cancelled.load(Ordering::SeqCst)
        && (manual
            || permits_run(
                settings,
                false,
                Local::now().hour(),
                runtime::idle_seconds(),
                true,
            ))
}

async fn ensure_tables(pool: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS ai_analysis (asset_id TEXT PRIMARY KEY, modified_at INTEGER NOT NULL, size_bytes INTEGER NOT NULL, preview TEXT NOT NULL, model TEXT NOT NULL, category TEXT NOT NULL, visual_hash TEXT, aspect REAL)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS ai_analysis_category ON ai_analysis(category)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS ai_suggestions (id TEXT PRIMARY KEY, payload TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', updated_at INTEGER NOT NULL)").execute(pool).await?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiItem {
    id: String,
    file_name: String,
    file_path: String,
    library_name: String,
    library_id: String,
    #[serde(default)]
    thumbnail_path: Option<String>,
    #[serde(default)]
    based_on: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSuggestion {
    id: String,
    kind: String,
    title: String,
    reason: String,
    asset_ids: Vec<String>,
    collection_name: Option<String>,
    status: String,
    items: Vec<AiItem>,
    #[serde(default)]
    total_items: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiBulkApplyResult {
    approved: usize,
    failed: usize,
}

async fn record_suggestion(pool: &sqlx::SqlitePool, mut suggestion: AiSuggestion) -> Result<()> {
    let _mutation = SUGGESTION_MUTATION.lock().await;
    suggestion
        .items
        .sort_by(|a, b| (&a.library_id, &a.id).cmp(&(&b.library_id, &b.id)));
    suggestion
        .items
        .dedup_by(|a, b| a.library_id == b.library_id && a.id == b.id);
    let items = std::mem::take(&mut suggestion.items);
    let base = suggestion.id.clone();
    let mut transaction = pool.begin().await?;
    let prefix = format!("{base}:");
    sqlx::query("DELETE FROM ai_suggestions WHERE substr(id,1,?)=? AND status='pending'")
        .bind(prefix.len() as i64)
        .bind(&prefix)
        .execute(&mut *transaction)
        .await?;
    for (page, items) in items.chunks(10000).enumerate() {
        suggestion.items = items.to_vec();
        suggestion.total_items = items.len();
        suggestion.asset_ids = items.iter().map(|item| item.id.clone()).collect();
        // Aynı öneri tekrar sunulmaz; yeni dosyalar eklendiğinde yeni üyelik kümesi incelenebilir.
        let mut fingerprint = 0xcbf29ce484222325u64;
        for item in items {
            for byte in format!("{}\0{}\0{}\0", item.library_id, item.id, item.file_path).bytes() {
                fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        suggestion.id = format!("{base}:page:{page}:{fingerprint:016x}");
        let payload = serde_json::to_string(&suggestion)?;
        sqlx::query("INSERT INTO ai_suggestions(id,payload,updated_at) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload WHERE ai_suggestions.status='pending'")
            .bind(&suggestion.id).bind(payload).bind(chrono::Utc::now().timestamp_millis()).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    activity::record("suggestion_prepared", serde_json::json!({ "id": base, "collection": suggestion.collection_name,
        "kind": suggestion.kind, "files": items.len() }));
    Ok(())
}

async fn collect_suggestions(db: &crate::db::Database, model: &str) -> Result<()> {
    retire_obsolete_similarity_data(db).await?;
    let active_prefix = format!("category:{model}:");
    sqlx::query("UPDATE ai_suggestions SET status='dismissed',updated_at=? WHERE status='pending' AND id LIKE 'category:%' AND substr(id,1,?)<>?")
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(active_prefix.len() as i64)
        .bind(&active_prefix)
        .execute(&db.master_pool).await?;
    let mut categories: HashMap<String, Vec<AiItem>> = HashMap::new();
    for library in db.get_all_libraries().await {
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='ai_analysis'",
        )
        .fetch_one(&library.pool)
        .await?;
        if exists == 0 {
            continue;
        }
        let rows = sqlx::query("SELECT a.id,a.file_name,a.file_path,c.category,a.thumbnail_path FROM ai_analysis c JOIN assets a ON a.id=c.asset_id AND a.modified_at=c.modified_at AND a.size_bytes=c.size_bytes WHERE c.model=? AND c.category<>'Other' ORDER BY a.id")
            .bind(model).fetch_all(&library.pool).await?;
        for row in rows {
            let item = AiItem {
                id: row.get(0),
                file_name: row.get(1),
                file_path: resolve_path(&library.root_path, row.get::<String, _>(2).as_str()),
                library_name: library.name.clone(),
                library_id: library.id.clone(),
                thumbnail_path: row.get::<Option<String>, _>(4).filter(|path| Path::new(path).starts_with(&library.catalog_dir)),
                based_on: None,
            };
            let category: String = row.get(3);
            categories.entry(category).or_default().push(item.clone());
        }
        // Var olan klasör+gövde eşleştirmesiyle renderın ZIP/MAX eşlerini de koleksiyona dahil et.
        let siblings = sqlx::query("SELECT DISTINCT a.id,a.file_name,a.file_path,c.category,COALESCE(a.thumbnail_path,source.thumbnail_path),source.file_name FROM ai_analysis c JOIN assets source ON source.id=c.asset_id AND source.modified_at=c.modified_at AND source.size_bytes=c.size_bytes JOIN asset_workflow w ON w.asset_id=c.asset_id JOIN asset_workflow sibling ON sibling.group_key=w.group_key JOIN assets a ON a.id=sibling.asset_id WHERE c.model=? AND c.category<>'Other' AND sibling.asset_id<>c.asset_id ORDER BY a.id")
            .bind(model).fetch_all(&library.pool).await?;
        for row in siblings {
            categories.entry(row.get(3)).or_default().push(AiItem {
                id: row.get(0),
                file_name: row.get(1),
                file_path: resolve_path(&library.root_path, row.get::<String, _>(2).as_str()),
                library_name: library.name.clone(),
                library_id: library.id.clone(),
                thumbnail_path: row.get::<Option<String>, _>(4).filter(|path| Path::new(path).starts_with(&library.catalog_dir)),
                based_on: row.get(5),
            });
        }
    }
    for (category, items) in categories {
        let name = if category == "Texture" {
            "Textures".to_string()
        } else {
            category.clone()
        };
        record_suggestion(&db.master_pool, AiSuggestion { id: format!("category:{model}:{category}"), kind: "collection".into(),
            title: format!("Group {name} assets across libraries"), reason: "Suggested from cached previews. Review the files; originals and model references will stay in place.".into(),
            asset_ids: vec![], collection_name: Some(name), status: "pending".into(), total_items: items.len(), items }).await?;
    }
    refresh_suggestion_count(&db.master_pool).await
}

async fn analyze(db: &crate::db::Database, settings: &AiSettings, manual: bool) -> Result<()> {
    ensure_tables(&db.master_pool).await?;
    let key = format!("v3:{}:{}", settings.provider, settings.model);
    let mut catalog_assets = 0usize;
    let mut pending_assets = 0usize;
    let mut without_preview = 0usize;
    for library in db.get_all_libraries().await {
        ensure_tables(&library.pool).await?;
        let (total, pending, missing): (i64, i64, i64) = sqlx::query_as("SELECT count(*),COALESCE(sum(a.thumbnail_path IS NOT NULL AND (c.asset_id IS NULL OR c.modified_at<>a.modified_at OR c.size_bytes<>a.size_bytes OR c.preview<>a.thumbnail_path OR c.model<>?)),0),COALESCE(sum(a.thumbnail_path IS NULL),0) FROM assets a LEFT JOIN ai_analysis c ON c.asset_id=a.id")
            .bind(&key).fetch_one(&library.pool).await?;
        catalog_assets += total as usize;
        pending_assets += pending as usize;
        without_preview += missing as usize;
    }
    update_status(|status| {
        status.catalog_assets = catalog_assets;
        status.pending_assets = pending_assets;
        status.without_preview = without_preview;
        status.analyzed_assets = catalog_assets.saturating_sub(pending_assets + without_preview);
        status.total = pending_assets.min(settings.batch_size as usize);
    });
    activity::record("catalog_summary", serde_json::json!({ "catalogAssets": catalog_assets, "pendingAssets": pending_assets,
        "withoutPreview": without_preview, "batchLimit": settings.batch_size }));
    let mut remaining = settings.batch_size as usize;
    let mut session: Option<runtime::Session> = None;
    let result = async {
        for library in db.get_all_libraries().await {
            ensure_tables(&library.pool).await?;
            let rows = sqlx::query("SELECT a.id,a.file_name,a.file_path,a.modified_at,a.size_bytes,a.thumbnail_path,a.extension FROM assets a LEFT JOIN ai_analysis c ON c.asset_id=a.id WHERE a.thumbnail_path IS NOT NULL AND (c.asset_id IS NULL OR c.modified_at<>a.modified_at OR c.size_bytes<>a.size_bytes OR c.preview<>a.thumbnail_path OR c.model<>?) ORDER BY a.id LIMIT ?")
                .bind(&key).bind(remaining as i64).fetch_all(&library.pool).await?;
            for row in rows {
                ensure!(should_continue(settings, manual), "Working conditions changed; analysis paused.");
                let id: String = row.get(0);
                let preview: String = row.get(5);
                let candidate = PathBuf::from(&preview);
                let catalog = tokio::fs::canonicalize(&library.catalog_dir).await?;
                // Eski NAS önizleme yollarını canonicalize ederek dahi uyandırma.
                let image_path = if candidate.is_absolute() && (candidate.starts_with(&library.catalog_dir) || candidate.starts_with(&catalog)) {
                    tokio::fs::canonicalize(&candidate).await.ok().filter(|path| path.starts_with(&catalog) && path.extension().is_some_and(|ext| ext != "glb"))
                } else { None };
                let category = if let Some(path) = &image_path {
                    if session.is_none() { session = Some(runtime::Session::connect(settings, manual).await?); }
                    update_status(|status| status.phase = format!("Classifying cached preview: {}", row.get::<String,_>(1)));
                    activity::record("asset_started", serde_json::json!({ "id": &id, "library": library.name,
                        "filePath": resolve_path(&library.root_path, row.get::<String,_>(2).as_str()), "model": settings.model }));
                    session.as_mut().unwrap().classify(settings, path, row.get::<String,_>(2), row.get::<String,_>(6).as_str(), manual).await?
                } else {
                    update_status(|status| status.skipped += 1);
                    "Other".into()
                };
                activity::record("asset_classified", serde_json::json!({ "id": &id, "fileName": row.get::<String,_>(1),
                    "filePath": resolve_path(&library.root_path, row.get::<String,_>(2).as_str()), "category": &category,
                    "basis": if image_path.is_some() { "cached_preview" } else { "skipped_missing_local_preview" } }));
                if image_path.is_some() && category != "Texture" {
                    remove_stale_texture_membership(&library.pool, &id).await?;
                }
                let hash = if let Some(path) = image_path.filter(|_| category != "Other") {
                    tokio::task::spawn_blocking(move || crate::duplicates::image_hash(&path).ok().map(|(hash, ratio)| (format!("{hash:016x}:{:.2}", ratio), ratio))).await?
                } else { None };
                sqlx::query("INSERT OR REPLACE INTO ai_analysis(asset_id,modified_at,size_bytes,preview,model,category,visual_hash,aspect) VALUES(?,?,?,?,?,?,?,?)")
                    .bind(&id).bind(row.get::<i64,_>(3)).bind(row.get::<i64,_>(4)).bind(&preview).bind(&key).bind(&category)
                    .bind(hash.as_ref().map(|(hash,_)| hash)).bind(hash.as_ref().map(|(_,ratio)| *ratio)).execute(&library.pool).await?;
                remaining -= 1; update_status(|status| {
                    status.processed += 1;
                    status.pending_assets = status.pending_assets.saturating_sub(1);
                    status.analyzed_assets += 1;
                });
            }
            if remaining == 0 { break; }
        }
        if let Some(session) = session.take() { session.close().await?; }
        if remaining < settings.batch_size as usize {
            update_status(|status| status.phase = "Preparing reviewable collection suggestions".into());
            collect_suggestions(db, &key).await?;
        }
        Ok(())
    }.await;
    if let Some(session) = session {
        let cleanup = session.close().await;
        if result.is_ok() {
            cleanup?;
        }
    }
    result
}

#[tauri::command]
pub async fn list_ai_suggestions(
    state: State<'_, Arc<AppState>>,
) -> std::result::Result<Vec<AiSuggestion>, String> {
    let removed = retire_obsolete_similarity_data(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    refresh_suggestion_count(&state.db.master_pool)
        .await
        .map_err(|error| error.to_string())?;
    if removed > 0 {
        let _ = state.app_handle.emit("library:updated", ());
    }
    reviewable_suggestions(&state.db)
        .await
        .map_err(|error| error.to_string())
}

async fn reviewable_suggestions(db: &crate::db::Database) -> Result<Vec<AiSuggestion>> {
    let mut suggestions = pending_suggestions(&db.master_pool).await?;
    // Önceden oluşturulmuş önerilerin kartları da yalnız yerel katalogdan tamamlanır.
    for library in db.get_all_libraries().await {
        for item in suggestions.iter_mut().flat_map(|suggestion| &mut suggestion.items).filter(|item| item.library_id == library.id) {
            if item.thumbnail_path.as_ref().is_some_and(|path| !Path::new(path).starts_with(&library.catalog_dir)) {
                item.thumbnail_path = None;
            }
        }
        let ids: std::collections::HashSet<_> = suggestions.iter().flat_map(|suggestion| suggestion.items.iter().take(4))
            .filter(|item| item.library_id == library.id && item.thumbnail_path.is_none()).map(|item| item.id.clone()).collect();
        let ids: Vec<_> = ids.into_iter().collect();
        let mut previews = HashMap::new();
        for ids in ids.chunks(400) {
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT a.id,a.thumbnail_path,source.thumbnail_path,source.file_name FROM assets a LEFT JOIN asset_workflow w ON w.asset_id=a.id LEFT JOIN asset_workflow sibling ON sibling.group_key=w.group_key AND sibling.asset_id<>a.id LEFT JOIN assets source ON source.id=sibling.asset_id AND source.thumbnail_path IS NOT NULL WHERE a.id IN (");
            let mut separated = query.separated(",");
            for id in ids { separated.push_bind(id); }
            query.push(") ORDER BY a.id,sibling.group_rank DESC,source.id");
            for row in query.build().fetch_all(&library.pool).await? {
                let own = row.get::<Option<String>, _>(1).filter(|path| Path::new(path).starts_with(&library.catalog_dir));
                let linked = row.get::<Option<String>, _>(2).filter(|path| Path::new(path).starts_with(&library.catalog_dir));
                let based_on = if own.is_none() { row.get::<Option<String>, _>(3) } else { None };
                if let Some(path) = own.or(linked) { previews.entry(row.get::<String, _>(0)).or_insert((path, based_on)); }
            }
        }
        for item in suggestions.iter_mut().flat_map(|suggestion| suggestion.items.iter_mut().take(4))
            .filter(|item| item.library_id == library.id && item.thumbnail_path.is_none()) {
            if let Some((path, based_on)) = previews.get(&item.id) {
                item.thumbnail_path = Some(path.clone());
                item.based_on = based_on.clone();
            }
        }
    }
    Ok(suggestions)
}

async fn retire_obsolete_similarity_data(db: &crate::db::Database) -> Result<usize> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='ai_suggestions'",
    )
    .fetch_one(&db.master_pool)
    .await?;
    let dismissed = if exists == 0 {
        0
    } else {
        sqlx::query("UPDATE ai_suggestions SET status='dismissed',updated_at=? WHERE status='pending' AND json_extract(payload,'$.kind')='duplicates'")
            .bind(chrono::Utc::now().timestamp_millis())
            .execute(&db.master_pool)
            .await?
            .rows_affected()
    };
    // Eski preview:{hash} önerileri her hash için ayrı, aynı adlı bir koleksiyon açıyordu.
    // Yalnız LumiaDAM'ın bu hatalı özellikle oluşturduğu kimlikleri temizle; dosyalara dokunma.
    let mut removed = sqlx::query("DELETE FROM collections WHERE id LIKE 'local-ai:preview:%'")
        .execute(&db.master_pool)
        .await?
        .rows_affected();
    for library in db.get_all_libraries().await {
        sqlx::query("DELETE FROM collection_assets WHERE collection_id LIKE 'local-ai:preview:%'")
            .execute(&library.pool)
            .await?;
        removed += sqlx::query("DELETE FROM collections WHERE id LIKE 'local-ai:preview:%'")
            .execute(&library.pool)
            .await?
            .rows_affected();
    }
    if dismissed > 0 || removed > 0 {
        activity::record("obsolete_similarity_data_cleaned", serde_json::json!({
            "suggestionsDismissed": dismissed,
            "collectionsRemoved": removed,
            "sourceFilesPreserved": true,
        }));
    }
    Ok(removed as usize)
}

async fn pending_suggestions(pool: &sqlx::SqlitePool) -> Result<Vec<AiSuggestion>> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='ai_suggestions'",
    )
    .fetch_one(pool)
    .await?;
    if exists == 0 {
        return Ok(vec![]);
    }
    // Tam üyelik yalnız onay sırasında okunur; panel kapalı modelin yerine RAM'i doldurmaz.
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT json_set(payload,'$.items',json((SELECT json_group_array(json(value)) FROM (SELECT value FROM json_each(s.payload,'$.items') LIMIT 100))),'$.assetIds',json('[]'),'$.totalItems',json_array_length(payload,'$.items')),status FROM ai_suggestions s WHERE status='pending' ORDER BY updated_at DESC LIMIT 200")
        .fetch_all(pool).await?;
    rows.into_iter()
        .map(|(payload, status)| {
            let mut item: AiSuggestion = serde_json::from_str(&payload)?;
            item.status = status;
            Ok(item)
        })
        .collect()
}

#[tauri::command]
pub async fn dismiss_ai_suggestion(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> std::result::Result<(), String> {
    let _mutation = SUGGESTION_MUTATION.lock().await;
    sqlx::query("UPDATE ai_suggestions SET status='dismissed' WHERE id=? AND status='pending'")
        .bind(&id)
        .execute(&state.db.master_pool)
        .await
        .map_err(|e| e.to_string())?;
    activity::record("suggestion_dismissed", serde_json::json!({ "id": id }));
    refresh_suggestion_count(&state.db.master_pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn apply_ai_suggestion(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> std::result::Result<(), String> {
    if let Err(error) = apply_suggestion(&state.db, &id).await {
        activity::record("approval_failed", serde_json::json!({ "id": id, "error": error.to_string() }));
        return Err(error.to_string());
    }
    refresh_suggestion_count(&state.db.master_pool)
        .await
        .map_err(|e| e.to_string())?;
    let _ = state.app_handle.emit("library:updated", ());
    Ok(())
}

#[tauri::command]
pub async fn apply_all_ai_suggestions(
    state: State<'_, Arc<AppState>>,
) -> std::result::Result<AiBulkApplyResult, String> {
    let removed = retire_obsolete_similarity_data(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    let result = apply_all_pending_suggestions(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    refresh_suggestion_count(&state.db.master_pool)
        .await
        .map_err(|error| error.to_string())?;
    if result.approved > 0 || removed > 0 {
        let _ = state.app_handle.emit("library:updated", ());
    }
    Ok(result)
}

async fn apply_all_pending_suggestions(db: &crate::db::Database) -> Result<AiBulkApplyResult> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM ai_suggestions WHERE status='pending' ORDER BY updated_at,id",
    )
    .fetch_all(&db.master_pool)
    .await?;
    let mut result = AiBulkApplyResult { approved: 0, failed: 0 };
    for id in ids {
        match apply_suggestion(db, &id).await {
            Ok(()) => result.approved += 1,
            Err(error) => {
                result.failed += 1;
                activity::record(
                    "bulk_approval_item_failed",
                    serde_json::json!({ "id": id, "error": error.to_string() }),
                );
            }
        }
    }
    activity::record("bulk_approval_finished", serde_json::json!({
        "approved": result.approved,
        "failed": result.failed,
    }));
    Ok(result)
}

async fn refresh_suggestion_count(pool: &sqlx::SqlitePool) -> Result<()> {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ai_suggestions WHERE status='pending'")
            .fetch_one(pool)
            .await?;
    update_status(|status| status.suggestions = count as usize);
    Ok(())
}

async fn apply_suggestion(db: &crate::db::Database, id: &str) -> Result<()> {
    let _mutation = SUGGESTION_MUTATION.lock().await;
    let row: (String, String) =
        sqlx::query_as("SELECT payload,status FROM ai_suggestions WHERE id=?")
            .bind(id)
            .fetch_one(&db.master_pool)
            .await?;
    ensure!(
        row.1 == "pending",
        "This suggestion has already been handled."
    );
    let suggestion: AiSuggestion = serde_json::from_str(&row.0)?;
    ensure!(
        ["collection", "duplicates"].contains(&suggestion.kind.as_str()),
        "Unsupported suggestion action."
    );
    let name = suggestion
        .collection_name
        .context("Suggestion has no collection name.")?;
    ensure!(
        name.len() <= 100 && !suggestion.items.is_empty() && suggestion.items.len() <= 10000,
        "Invalid suggestion size."
    );
    let collection_id = format!(
        "local-ai:{}",
        id.split_once(":page:")
            .context("Invalid suggestion identifier.")?
            .0
    );
    let libraries = db.get_all_libraries().await;
    // Kullanıcıya gösterilen dosya ve kütüphane kimlikleri uygulanmadan önce tekrar doğrulanır.
    for item in &suggestion.items {
        let library = libraries
            .iter()
            .find(|lib| lib.id == item.library_id)
            .context("A source library is no longer available.")?;
        let path: Option<String> = sqlx::query_scalar("SELECT file_path FROM assets WHERE id=?")
            .bind(&item.id)
            .fetch_optional(&library.pool)
            .await?;
        ensure!(
            path.as_ref()
                .is_some_and(|path| resolve_path(&library.root_path, path) == item.file_path),
            "A proposed file changed. Run analysis again."
        );
    }
    // Sabit kimlik, kesintiden sonra yeniden denemede ikinci bir koleksiyon oluşturmaz.
    let create = "INSERT OR IGNORE INTO collections(id,name,description,is_smart,created_at) VALUES(?,?,?,0,?)";
    let now = chrono::Utc::now().timestamp_millis();
    let mut master = db.master_pool.begin().await?;
    sqlx::query(create)
        .bind(&collection_id)
        .bind(&name)
        .bind(&suggestion.reason)
        .bind(now)
        .execute(&mut *master)
        .await?;
    for library in libraries {
        let members: Vec<_> = suggestion
            .items
            .iter()
            .filter(|item| item.library_id == library.id)
            .collect();
        if members.is_empty() {
            continue;
        }
        let mut transaction = library.pool.begin().await?;
        sqlx::query(create)
            .bind(&collection_id)
            .bind(&name)
            .bind(&suggestion.reason)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        for item in members {
            sqlx::query(
                "INSERT OR IGNORE INTO collection_assets(collection_id,asset_id) VALUES(?,?)",
            )
            .bind(&collection_id)
            .bind(&item.id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
    }
    let changed =
        sqlx::query("UPDATE ai_suggestions SET status='approved' WHERE id=? AND status='pending'")
            .bind(id)
            .execute(&mut *master)
            .await?
            .rows_affected();
    ensure!(
        changed == 1,
        "The suggestion changed during approval. Refresh the suggestions."
    );
    master.commit().await?;
    activity::record("collection_approved", serde_json::json!({ "id": id, "collection": name,
        "files": suggestion.items.len(), "sourceFilesPreserved": true }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_are_only_allowed_for_image_assets() {
        assert!(texture_allowed_for("jpg"));
        assert!(texture_allowed_for(".EXR"));
        assert!(!texture_allowed_for("zip"));
        assert!(!texture_allowed_for("max"));
    }

    #[tokio::test]
    async fn reclassification_removes_only_ai_texture_collection_membership() {
        let root = std::env::temp_dir().join(format!("lumiadam-ai-texture-reclass-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("NAS")).unwrap();
        let db = crate::db::Database::new(root.join("master.sqlite").to_str().unwrap(), root.join("catalogs")).await.unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let library = db.register_library(&id, "NAS", &root.join("NAS")).await.unwrap();
        for (collection, name) in [("local-ai:category:Texture", "Textures"), ("custom", "My collection")] {
            sqlx::query("INSERT INTO collections(id,name,is_smart,created_at) VALUES(?,?,0,1)")
                .bind(collection).bind(name).execute(&library.pool).await.unwrap();
        }
        sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES('asset','decor.zip','decor.zip','zip','archive',1,1,1)")
            .execute(&library.pool).await.unwrap();
        sqlx::query("INSERT INTO collection_assets(collection_id,asset_id) VALUES('local-ai:category:Texture','asset'),('custom','asset')")
            .execute(&library.pool).await.unwrap();

        remove_stale_texture_membership(&library.pool, "asset").await.unwrap();

        let remaining: Vec<String> = sqlx::query_scalar("SELECT collection_id FROM collection_assets WHERE asset_id='asset'")
            .fetch_all(&library.pool).await.unwrap();
        assert_eq!(remaining, ["custom"]);
        for library in db.get_all_libraries().await { library.pool.close().await; }
        db.master_pool.close().await;
        drop(library);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saved_settings_survive_replacement_without_starting_any_service() {
        let path =
            std::env::temp_dir().join(format!("lumiadam-ai-settings-{}", uuid::Uuid::new_v4()));
        assert_eq!(read_settings_at(&path).unwrap().mode, "disabled");
        let mut settings = AiSettings::default();
        save_settings_at(&path, &settings).unwrap();
        settings.mode = "scheduled".into();
        settings.setup_complete = true;
        save_settings_at(&path, &settings).unwrap();
        assert_eq!(read_settings_at(&path).unwrap().mode, "scheduled");
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
        assert!(!path.join("runtime").exists());
        std::fs::remove_dir_all(path).unwrap();
    }

    fn suggestion(library_id: &str, id: &str, file_path: String) -> AiSuggestion {
        AiSuggestion {
            id: "category:Kitchen".into(),
            kind: "collection".into(),
            title: "Kitchen".into(),
            reason: "Review cached previews".into(),
            asset_ids: vec![],
            collection_name: Some("Kitchen".into()),
            status: "pending".into(),
            total_items: 1,
            items: vec![AiItem {
                id: id.into(),
                file_name: "9.jpg".into(),
                file_path,
                library_name: "NAS".into(),
                library_id: library_id.into(),
                thumbnail_path: None,
                based_on: None,
            }],
        }
    }

    #[tokio::test]
    async fn cached_preview_similarity_does_not_create_review_collections() {
        let root = std::env::temp_dir().join(format!("lumiadam-ai-similar-previews-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("NAS")).unwrap();
        let db = crate::db::Database::new(
            root.join("master.sqlite").to_str().unwrap(),
            root.join("catalogs"),
        ).await.unwrap();
        ensure_tables(&db.master_pool).await.unwrap();
        let library_id = uuid::Uuid::new_v4().to_string();
        let library = db.register_library(&library_id, "NAS", &root.join("NAS")).await.unwrap();
        ensure_tables(&library.pool).await.unwrap();
        for (id, name) in [("a", "33.jpg"), ("b", "33.jpeg")] {
            sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES(?,?,?, 'jpg','image',1,1,1)")
                .bind(id).bind(format!("renders/{name}")).bind(name).execute(&library.pool).await.unwrap();
            sqlx::query("INSERT INTO ai_analysis(asset_id,modified_at,size_bytes,preview,model,category,visual_hash,aspect) VALUES(?,1,1,'preview','model','Living room','same-perceptual-hash:1.00',1.0)")
                .bind(id).execute(&library.pool).await.unwrap();
        }
        for pool in [&db.master_pool, &library.pool] {
            sqlx::query("INSERT INTO collections(id,name,description,is_smart,created_at) VALUES('local-ai:preview:old','Review: matching previews','old preview hash',0,1)")
                .execute(pool).await.unwrap();
        }
        sqlx::query("INSERT INTO collection_assets(collection_id,asset_id) VALUES('local-ai:preview:old','a')")
            .execute(&library.pool).await.unwrap();
        let mut old_similarity = suggestion(&library_id, "a", resolve_path(&library.root_path, "renders/33.jpg"));
        old_similarity.id = "preview:legacy".into();
        old_similarity.kind = "duplicates".into();
        old_similarity.collection_name = Some("Review: matching previews".into());
        record_suggestion(&db.master_pool, old_similarity).await.unwrap();

        collect_suggestions(&db, "model").await.unwrap();

        let payloads: Vec<String> = sqlx::query_scalar("SELECT payload FROM ai_suggestions WHERE status='pending' ORDER BY id")
            .fetch_all(&db.master_pool).await.unwrap();
        assert_eq!(payloads.len(), 1, "similar preview hashes must not create a second Review collection");
        let suggestion: AiSuggestion = serde_json::from_str(&payloads[0]).unwrap();
        assert_eq!(suggestion.kind, "collection");
        assert_eq!(suggestion.collection_name.as_deref(), Some("Living room"));
        assert_eq!(suggestion.items.len(), 2);
        let dismissed: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_suggestions WHERE status='dismissed'")
            .fetch_one(&db.master_pool).await.unwrap();
        assert_eq!(dismissed, 1, "legacy preview-review proposals are retired when new suggestions are prepared");
        let old_master: i64 = sqlx::query_scalar("SELECT count(*) FROM collections WHERE id='local-ai:preview:old'")
            .fetch_one(&db.master_pool).await.unwrap();
        let old_library: i64 = sqlx::query_scalar("SELECT count(*) FROM collections WHERE id='local-ai:preview:old'")
            .fetch_one(&library.pool).await.unwrap();
        let old_members: i64 = sqlx::query_scalar("SELECT count(*) FROM collection_assets WHERE collection_id='local-ai:preview:old'")
            .fetch_one(&library.pool).await.unwrap();
        assert_eq!((old_master, old_library, old_members), (0, 0, 0));

        library.pool.close().await;
        db.master_pool.close().await;
        db.pool.close().await;
        db.libraries.write().await.clear();
        drop(library);
        drop(db);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn bulk_approval_handles_every_pending_suggestion_and_reports_failures() {
        let root = std::env::temp_dir().join(format!("lumiadam-ai-bulk-approval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("NAS")).unwrap();
        let db = crate::db::Database::new(root.join("master.sqlite").to_str().unwrap(), root.join("catalogs"))
            .await.unwrap();
        ensure_tables(&db.master_pool).await.unwrap();
        let library_id = uuid::Uuid::new_v4().to_string();
        let library = db.register_library(&library_id, "NAS", &root.join("NAS")).await.unwrap();
        for (id, file_path, file_name) in [("kitchen", "kitchen.jpg", "kitchen.jpg"), ("bedroom", "bedroom.jpg", "bedroom.jpg")] {
            sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES(?,?,?,'jpg','image',1,1,1)")
                .bind(id).bind(file_path).bind(file_name).execute(&library.pool).await.unwrap();
            let mut proposal = suggestion(&library_id, id, resolve_path(&library.root_path, file_path));
            proposal.id = format!("category:{id}");
            proposal.collection_name = Some(id.to_string());
            record_suggestion(&db.master_pool, proposal).await.unwrap();
        }
        let mut unavailable = suggestion("missing-library", "gone", "Z:/gone.jpg".into());
        unavailable.id = "category:unavailable".into();
        unavailable.collection_name = Some("Unavailable".into());
        record_suggestion(&db.master_pool, unavailable).await.unwrap();

        let result = apply_all_pending_suggestions(&db).await.unwrap();

        assert_eq!((result.approved, result.failed), (2, 1));
        let collections: i64 = sqlx::query_scalar("SELECT count(*) FROM collections WHERE id LIKE 'local-ai:%'")
            .fetch_one(&db.master_pool).await.unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_suggestions WHERE status='pending'")
            .fetch_one(&db.master_pool).await.unwrap();
        assert_eq!((collections, remaining), (2, 1));
        library.pool.close().await;
        db.master_pool.close().await;
        db.pool.close().await;
        db.libraries.write().await.clear();
        drop(library);
        drop(db);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn reviewed_suggestions_stay_reviewed_and_new_members_get_a_new_review() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        ensure_tables(&pool).await.unwrap();
        let item = suggestion("lib", "asset", "A:/kitchen/9.jpg".into());
        record_suggestion(&pool, item.clone()).await.unwrap();
        sqlx::query("UPDATE ai_suggestions SET status='dismissed'")
            .execute(&pool)
            .await
            .unwrap();
        record_suggestion(&pool, item.clone()).await.unwrap();
        let pending: i64 =
            sqlx::query_scalar("SELECT count(*) FROM ai_suggestions WHERE status='pending'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pending, 0);
        let mut expanded = item;
        expanded.items.push(AiItem {
            id: "new".into(),
            ..expanded.items[0].clone()
        });
        record_suggestion(&pool, expanded.clone()).await.unwrap();
        expanded.items.push(AiItem {
            id: "third".into(),
            ..expanded.items[0].clone()
        });
        record_suggestion(&pool, expanded).await.unwrap();
        let counts: (i64, i64) =
            sqlx::query_as("SELECT count(*),sum(status='pending') FROM ai_suggestions")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(counts, (2, 1));
        pool.close().await;
    }

    #[tokio::test]
    async fn approval_is_offline_idempotent_and_checks_the_reviewed_path() {
        let root =
            std::env::temp_dir().join(format!("lumiadam-ai-approval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("NAS/kitchen")).unwrap();
        let db = crate::db::Database::new(
            root.join("master.sqlite").to_str().unwrap(),
            root.join("catalogs"),
        )
        .await
        .unwrap();
        let library_id = uuid::Uuid::new_v4().to_string();
        let library = db
            .register_library(&library_id, "NAS", &root.join("NAS"))
            .await
            .unwrap();
        ensure_tables(&db.master_pool).await.unwrap();
        sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES('image','kitchen/9.jpg','9.jpg','jpg','texture',1,1,1)").execute(&library.pool).await.unwrap();
        let source = root.join("NAS/kitchen/9.jpg");
        std::fs::write(&source, b"original").unwrap();
        let proposal = suggestion(
            &library_id,
            "image",
            resolve_path(&library.root_path, "kitchen/9.jpg"),
        );
        record_suggestion(&db.master_pool, proposal).await.unwrap();
        let id: String = sqlx::query_scalar("SELECT id FROM ai_suggestions")
            .fetch_one(&db.master_pool)
            .await
            .unwrap();
        sqlx::query("UPDATE assets SET file_path='renamed.jpg'")
            .execute(&library.pool)
            .await
            .unwrap();
        assert!(apply_suggestion(&db, &id).await.is_err());
        sqlx::query("UPDATE assets SET file_path='kitchen/9.jpg'")
            .execute(&library.pool)
            .await
            .unwrap();
        std::fs::rename(root.join("NAS"), root.join("NAS-offline")).unwrap();
        apply_suggestion(&db, &id).await.unwrap();
        assert!(apply_suggestion(&db, &id).await.is_err());
        assert_eq!(
            std::fs::read(root.join("NAS-offline/kitchen/9.jpg")).unwrap(),
            b"original"
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM collections WHERE id LIKE 'local-ai:%'")
                .fetch_one(&db.master_pool)
                .await
                .unwrap();
        let members: i64 = sqlx::query_scalar("SELECT count(*) FROM collection_assets")
            .fetch_one(&library.pool)
            .await
            .unwrap();
        assert_eq!((count, members), (1, 1));
        std::fs::create_dir(root.join("missing-NAS")).unwrap();
        let untouched = db
            .register_library(
                &uuid::Uuid::new_v4().to_string(),
                "Unanalyzed",
                &root.join("missing-NAS"),
            )
            .await
            .unwrap();
        collect_suggestions(&db, "not-yet-analyzed").await.unwrap();
        untouched.pool.close().await;
        library.pool.close().await;
        db.master_pool.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn large_categories_are_split_into_reviewable_proposals_without_losing_members() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        ensure_tables(&pool).await.unwrap();
        let mut item = suggestion("lib", "0", "A:/0.jpg".into());
        item.items = (0..10001)
            .map(|index| AiItem {
                id: format!("{index:05}"),
                ..item.items[0].clone()
            })
            .collect();
        record_suggestion(&pool, item).await.unwrap();
        let payloads: Vec<String> = sqlx::query_scalar("SELECT payload FROM ai_suggestions")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(payloads.len(), 2);
        let lengths: Vec<usize> = payloads
            .iter()
            .map(|payload| {
                serde_json::from_str::<AiSuggestion>(payload)
                    .unwrap()
                    .items
                    .len()
            })
            .collect();
        assert!(lengths.iter().all(|length| *length <= 10000));
        assert_eq!(lengths.iter().sum::<usize>(), 10001);
        let visible = pending_suggestions(&pool).await.unwrap();
        assert_eq!(
            visible.iter().map(|item| item.total_items).sum::<usize>(),
            10001
        );
        assert!(visible
            .iter()
            .all(|item| item.items.len() <= 100 && item.asset_ids.is_empty()));
        assert!(visible
            .iter()
            .any(|item| item.total_items == 10000 && item.items.len() == 100));
        pool.close().await;
    }

    #[test]
    fn first_install_has_no_work_permission() {
        let settings = AiSettings::default();
        assert_eq!(settings.mode, "disabled");
        assert!(!settings.setup_complete);
        assert!(!permits_run(&settings, true, 3, 3600, true));
    }

    #[tokio::test]
    async fn analysis_uses_only_local_cache_reuses_results_and_groups_model_siblings() {
        let root = std::env::temp_dir().join(format!("lumiadam-ai-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("NAS/kitchen")).unwrap();
        let db = crate::db::Database::new(
            root.join("master.sqlite").to_str().unwrap(),
            root.join("catalogs"),
        )
        .await
        .unwrap();
        let library = db
            .register_library(&uuid::Uuid::new_v4().to_string(), "NAS", &root.join("NAS"))
            .await
            .unwrap();
        for (id, extension, kind) in [("image", "jpg", "texture"), ("model", "zip", "other")] {
            let path = format!("kitchen/9.{extension}");
            sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES(?,?,?,?,?,1,1,1)")
                .bind(id).bind(&path).bind(format!("9.{extension}")).bind(extension).bind(kind).execute(&library.pool).await.unwrap();
            let (directory, key, rank) = crate::workflow::group_key(&path, extension, kind);
            sqlx::query("INSERT INTO asset_workflow(asset_id,file_path,directory,group_key,group_rank) VALUES(?,?,?,?,?)")
                .bind(id).bind(path).bind(directory).bind(key).bind(rank).execute(&library.pool).await.unwrap();
        }
        let preview = library.catalog_dir.join("thumbs/preview.png");
        image::RgbImage::new(16, 16).save(&preview).unwrap();
        sqlx::query("UPDATE assets SET thumbnail_path=? WHERE id='image'")
            .bind(preview.to_string_lossy().as_ref())
            .execute(&library.pool)
            .await
            .unwrap();
        // NAS'ın kendisi erişilemez; analizin yerel katalog ve önizlemeyle bitmesi gerekir.
        std::fs::remove_dir_all(root.join("NAS")).unwrap();
        let (endpoint, task) = runtime::tests::server(vec![
            ("/api/show", serde_json::json!({"capabilities":["vision"]})),
            ("/api/ps", serde_json::json!({"models":[]})),
            (
                "/api/chat",
                serde_json::json!({"message":{"content":"{\"category\":\"Kitchen\",\"confidence\":0.95}"}}),
            ),
            ("/api/generate", serde_json::json!({"done":true})),
        ])
        .await;
        let settings = AiSettings {
            provider: "ollama".into(),
            endpoint,
            mode: "manual".into(),
            setup_complete: true,
            ..Default::default()
        };
        let outdated = suggestion("legacy-library", "legacy-asset", "A:/old-room.jpg".into());
        ensure_tables(&db.master_pool).await.unwrap();
        record_suggestion(&db.master_pool, outdated).await.unwrap();
        analyze(&db, &settings, true).await.unwrap();
        let old_status: String = sqlx::query_scalar("SELECT status FROM ai_suggestions WHERE id LIKE 'category:Kitchen:%'")
            .fetch_one(&db.master_pool).await.unwrap();
        assert_eq!(old_status, "dismissed", "eski yanlış sınıflandırma önerileri yeni analizden sonra görünmemeli");
        let status = get_local_ai_status();
        assert_eq!((status.catalog_assets, status.analyzed_assets, status.pending_assets, status.without_preview), (2, 1, 0, 1));
        let log = serde_json::to_value(activity::read().unwrap()).unwrap();
        assert!(log["entries"].as_array().unwrap().iter().any(|entry| entry["event"] == "asset_classified" && entry["details"]["category"] == "Kitchen"));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
                .len(),
            4
        );
        // Sahte sunucu artık kapalı: yeniden analiz model bağlantısı kurarsa bu çağrı hata verir.
        analyze(&db, &settings, true).await.unwrap();
        let payload: String = sqlx::query_scalar(
            "SELECT payload FROM ai_suggestions WHERE id LIKE 'category:v3:ollama:%:Kitchen:%'",
        )
        .fetch_one(&db.master_pool)
        .await
        .unwrap();
        let suggestion: AiSuggestion = serde_json::from_str(&payload).unwrap();
        assert_eq!(suggestion.asset_ids.len(), 2);
        assert!(suggestion.asset_ids.contains(&"model".into()));
        // Eski kayıt biçimi: küçük resim alanları yokken ve NAS kapalıyken de kartlar tamamlanmalı.
        sqlx::query("UPDATE ai_suggestions SET payload=json_remove(payload,'$.items[0].thumbnailPath','$.items[1].thumbnailPath')")
            .execute(&db.master_pool).await.unwrap();
        let visible = reviewable_suggestions(&db).await.unwrap();
        assert!(visible[0].items.iter().all(|item| item.thumbnail_path.as_deref() == Some(preview.to_string_lossy().as_ref())));
        assert_eq!(visible[0].items.iter().find(|item| item.id == "model").unwrap().based_on.as_deref(), Some("9.jpg"));
        let collections: i64 = sqlx::query_scalar("SELECT count(*) FROM collections")
            .fetch_one(&db.master_pool)
            .await
            .unwrap();
        assert_eq!(collections, 0);
        library.pool.close().await;
        db.master_pool.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_mode_only_runs_after_explicit_action() {
        let settings = AiSettings {
            mode: "manual".into(),
            setup_complete: true,
            ..Default::default()
        };
        assert!(!permits_run(&settings, false, 3, 3600, true));
        assert!(permits_run(&settings, true, 3, 0, false));
    }

    #[test]
    fn scheduled_hours_support_midnight_and_never_run_outside_window() {
        let settings = AiSettings {
            mode: "scheduled".into(),
            start_hour: 22,
            end_hour: 6,
            setup_complete: true,
            ..Default::default()
        };
        for hour in [22, 23, 0, 5] {
            assert!(permits_run(&settings, false, hour, 0, true));
        }
        for hour in [6, 12, 21] {
            assert!(!permits_run(&settings, false, hour, 3600, true));
        }
    }

    #[test]
    fn idle_mode_requires_both_inactivity_and_low_machine_load() {
        let settings = AiSettings {
            mode: "idle".into(),
            setup_complete: true,
            ..Default::default()
        };
        assert!(!permits_run(&settings, false, 3, 599, true));
        assert!(!permits_run(&settings, false, 3, 3600, false));
        assert!(permits_run(&settings, false, 3, 600, true));
    }

    #[test]
    fn incomplete_setup_blocks_saved_automation() {
        let settings = AiSettings {
            mode: "idle".into(),
            ..Default::default()
        };
        assert!(!permits_run(&settings, false, 3, 3600, true));
    }

    #[test]
    fn endpoints_cannot_send_library_previews_to_remote_hosts() {
        assert_eq!(
            local_endpoint("http://localhost:11434").unwrap(),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            local_endpoint("http://127.0.0.1:1234/").unwrap(),
            "http://127.0.0.1:1234"
        );
        for endpoint in [
            "https://example.com",
            "http://192.168.1.5:11434",
            "http://user:password@127.0.0.1",
            "http://127.0.0.1/path",
            "file:///C:/data",
        ] {
            assert!(local_endpoint(endpoint).is_err(), "{endpoint}");
        }
    }

    #[test]
    fn model_output_is_only_a_category_never_an_executable_plan() {
        assert_eq!(parse_category(r#"{"category":"Kitchen","confidence":0.95}"#).unwrap(), "Kitchen");
        assert_eq!(parse_category(r#"{"category":"Bedroom","confidence":0.52}"#).unwrap(), "Other");
        assert_eq!(parse_category(r#"{"category":"Dressing room","confidence":0.93}"#).unwrap(), "Dressing room");
        for value in [
            r#"{"category":"../../outside"}"#,
            r#"{"category":"Kitchen","code":"delete"}"#,
            r#"{"category":"Kitchen","confidence":1.5}"#,
            "not JSON",
        ] {
            assert!(parse_category(value).is_err());
        }
    }
}
