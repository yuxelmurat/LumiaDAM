//! Yerel AI işlemlerinin sınırlı boyutlu, satır başına JSON içeren tanılama günlüğü.
use super::{ai_root, AiStatus};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs::{self, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, path::{Path, PathBuf}, sync::Mutex};

const MAX_BYTES: u64 = 5 * 1024 * 1024;
const TAIL_BYTES: u64 = 256 * 1024;
static LOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
pub struct Entry {
    at: String,
    event: String,
    details: Value,
}

#[derive(Serialize)]
pub struct AiLog {
    pub path: String,
    entries: Vec<Entry>,
}

fn root() -> Result<PathBuf> {
    // Testler kullanıcı günlüğünü değiştirmez.
    if cfg!(test) {
        return Ok(std::env::temp_dir().join(format!("lumiadam-ai-log-tests-{}", std::process::id())));
    }
    ai_root()
}

fn append_at(root: &Path, entry: &Entry, limit: u64) -> Result<()> {
    fs::create_dir_all(root)?;
    let path = root.join("activity.jsonl");
    let mut bytes = serde_json::to_vec(entry)?;
    ensure!(bytes.len() < TAIL_BYTES as usize, "The Local AI log entry is too large.");
    bytes.push(b'\n');
    if path.metadata().map(|meta| meta.len()).unwrap_or(0) + bytes.len() as u64 > limit {
        // En fazla dört eski dosya tutulur; günlük toplamda yaklaşık 25 MiB ile sınırlıdır.
        for index in (1..=4).rev() {
            let previous = if index == 1 { path.clone() } else { root.join(format!("activity.{}.jsonl", index - 1)) };
            if previous.exists() { fs::rename(previous, root.join(format!("activity.{index}.jsonl")))?; }
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).read(true).open(path)?;
    if file.metadata()?.len() > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' { file.write_all(b"\n")?; }
    }
    file.write_all(&bytes)?;
    Ok(())
}

pub(super) fn record(event: &str, details: Value) {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let entry = Entry { at: chrono::Utc::now().to_rfc3339(), event: event.into(), details };
    if let Err(error) = root().and_then(|root| append_at(&root, &entry, MAX_BYTES)) {
        tracing::warn!(%error, "Local AI activity could not be written");
    }
}

fn read_at(root: &Path) -> Result<AiLog> {
    fs::create_dir_all(root)?;
    let path = root.join("activity.jsonl");
    OpenOptions::new().create(true).append(true).open(&path)?;
    let mut entries = Vec::new();
    // Döndürme sonrasında önceki işlem kaydı da görünür; tüm dosyalar belleğe yüklenmez.
    for file in [root.join("activity.1.jsonl"), path.clone()] {
        if !file.exists() { continue; }
        let mut file = fs::File::open(file)?;
        let offset = file.metadata()?.len().saturating_sub(TAIL_BYTES);
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(TAIL_BYTES).read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        for (index, line) in text.lines().enumerate() {
            if offset > 0 && index == 0 { continue; }
            // Kesilmiş son satır sonraki kayıtların okunmasını engellemez.
            if let Ok(entry) = serde_json::from_str::<Entry>(line) { entries.push(entry); }
        }
    }
    entries.reverse();
    entries.truncate(200);
    Ok(AiLog { path: path.to_string_lossy().into_owned(), entries })
}

pub(super) fn read() -> Result<AiLog> {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_at(&root()?)
}

pub(super) fn restore_status() -> Option<AiStatus> {
    // Kayıt yokken uygulama açılışı bir günlük dosyası oluşturmaz.
    let root = root().ok()?;
    if !root.join("activity.jsonl").is_file() { return None; }
    status_from(read().ok()?)
}

fn status_from(log: AiLog) -> Option<AiStatus> {
    let mut status: AiStatus = log.entries.into_iter()
        .find_map(|entry| serde_json::from_value(entry.details.get("status")?.clone()).ok())?;
    if status.running {
        status.state = "paused".into();
        status.phase = "The previous operation was interrupted. Run again to continue with remaining assets.".into();
    }
    status.running = false;
    status.engine_running = false;
    status.model_loaded = false;
    Some(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_survives_restart_preserves_unicode_and_handles_partial_lines() {
        let root = std::env::temp_dir().join(format!("lumiadam-ai-log-{}", uuid::Uuid::new_v4()));
        let entry = Entry { at: "2026-10-15T12:00:00Z".into(), event: "asset_classified".into(),
            details: serde_json::json!({"path":"A:/Mutfak/立面图/9.jpg", "category":"Kitchen", "message":"line\nnext"}) };
        append_at(&root, &entry, MAX_BYTES).unwrap();
        OpenOptions::new().append(true).open(root.join("activity.jsonl")).unwrap().write_all(b"{broken").unwrap();
        append_at(&root, &entry, MAX_BYTES).unwrap();
        let log = read_at(&root).unwrap();
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[0].details, entry.details);
        assert!(log.path.ends_with("activity.jsonl"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rotation_keeps_bounded_files_and_reads_the_latest_events_first() {
        let root = std::env::temp_dir().join(format!("lumiadam-ai-log-{}", uuid::Uuid::new_v4()));
        for index in 0..20 {
            append_at(&root, &Entry { at: index.to_string(), event: "test".into(), details: serde_json::json!({"index": index}) }, 140).unwrap();
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 5);
        let log = read_at(&root).unwrap();
        assert_eq!(log.entries[0].details["index"], 19);
        assert!(log.entries.len() >= 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_run_is_restored_as_paused_without_starting_a_model() {
        let status = AiStatus { state: "running".into(), running: true, engine_running: true, model_loaded: true,
            catalog_assets: 1000, analyzed_assets: 120, pending_assets: 680, without_preview: 200,
            ..Default::default() };
        let log = AiLog { path: String::new(), entries: vec![Entry { at: String::new(), event: "status_changed".into(),
            details: serde_json::json!({"status": status}) }] };
        let restored = status_from(log).unwrap();
        assert_eq!(restored.state, "paused");
        assert_eq!(restored.pending_assets, 680);
        assert!(!restored.running && !restored.engine_running && !restored.model_loaded);
    }
}
