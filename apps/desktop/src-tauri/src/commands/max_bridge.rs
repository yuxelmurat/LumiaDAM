use crate::{workflow, AppState};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;
use tokio::fs;
use uuid::Uuid;

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaxInstance {
    pub id: String,
    pub version: u32,
    pub pid: u32,
    pub max_version: String,
    pub scene_path: String,
    pub project_path: String,
    pub busy: bool,
    pub updated_at: f64,
    #[serde(default = "legacy_merge_extensions")]
    pub merge_extensions: Vec<String>,
}

fn legacy_merge_extensions() -> Vec<String> {
    vec!["max".into(), "zip".into()]
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn bridge_root() -> Result<PathBuf> {
    let local =
        std::env::var_os("LOCALAPPDATA").ok_or("Windows local application data is unavailable.")?;
    Ok(PathBuf::from(local)
        .join("LumiaDAM")
        .join("bridge")
        .join("3dsmax"))
}

fn uuid_path(root: &Path, id: &str) -> Result<PathBuf> {
    let id = Uuid::parse_str(id).map_err(|_| "Invalid bridge identifier.")?;
    Ok(root.join(id.to_string()))
}

fn supports_merge_source(source: &Path) -> bool {
    source.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("max") || extension.eq_ignore_ascii_case("zip") || extension.eq_ignore_ascii_case("rar")
    })
}

async fn read_json(path: &Path) -> Result<serde_json::Value> {
    if fs::metadata(path).await.map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("Bridge response is too large.".into());
    }
    serde_json::from_slice(&fs::read(path).await.map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn is_live(instance: &MaxInstance, current_time: f64) -> bool {
    instance.version == 1
        && instance.pid > 0
        && instance.updated_at.is_finite()
        && (-5.0..15.0).contains(&(current_time - instance.updated_at))
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows::Win32::{
        Foundation::{CloseHandle, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // Kalp atışı merge sırasında durabilir; süreç kontrolü bunu kapanma saymaz.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut exit_code = 0;
        let queried = GetExitCodeProcess(handle, &mut exit_code).is_ok();
        let _ = CloseHandle(handle);
        !queried || exit_code == STILL_ACTIVE.0 as u32
    }
}

async fn read_instance(root: &Path, id: &str) -> Result<MaxInstance> {
    let directory = uuid_path(root, id)?;
    let instance: MaxInstance =
        serde_json::from_value(read_json(&directory.join("heartbeat.json")).await?)
            .map_err(|e| e.to_string())?;
    if instance.id != id || !is_live(&instance, now()) {
        return Err("The 3ds Max plugin is not responding. Start or restart 3ds Max.".into());
    }
    Ok(instance)
}

#[tauri::command]
pub async fn get_max_instances() -> Result<Vec<MaxInstance>> {
    let root = bridge_root()?;
    let mut instances = Vec::new();
    let mut entries = match fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(instances),
        Err(error) => return Err(error.to_string()),
    };
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let id = entry.file_name().to_string_lossy().into_owned();
        if let Ok(instance) = read_instance(&root, &id).await {
            instances.push(instance);
        }
    }
    instances.sort_by_key(|instance| instance.pid);
    Ok(instances)
}

#[tauri::command]
pub async fn merge_to_max(
    asset_id: String,
    instance_id: String,
    scene_path: String,
    project_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String> {
    let root = bridge_root()?;
    let instance = read_instance(&root, &instance_id).await?;
    if instance.busy {
        return Err("3ds Max is already processing a LumiaDAM request.".into());
    }
    if instance.scene_path != scene_path {
        return Err("The target scene changed. Select the Max scene again.".into());
    }
    let project = PathBuf::from(&project_path);
    if !project.is_absolute() || !fs::metadata(&project).await.is_ok_and(|m| m.is_dir()) {
        return Err("Select an existing project folder.".into());
    }
    // Kaynak, mevcut yerel kopya çözümleyicisiyle bulunur; arayüz ham kaynak yolu göndermez.
    let source = workflow::readable_path(&state.db, &asset_id)
        .await
        .map_err(|e| e.to_string())?;
    if !supports_merge_source(&source) {
        return Err("Only MAX files and ZIP/RAR archives can be merged.".into());
    }
    if source.extension().is_some_and(|extension| !instance.merge_extensions.iter().any(|supported| extension.eq_ignore_ascii_case(supported))) {
        return Err("The running 3ds Max plugin does not support this archive format. Reinstall LumiaDAM with the 3ds Max plugin selected, then restart 3ds Max.".into());
    }
    let directory = uuid_path(&root, &instance_id)?;
    let id = Uuid::new_v4().to_string();
    let request = serde_json::json!({"version": 1, "action": "merge", "id": id,
        "assetId": asset_id, "sourcePath": source, "projectPath": project,
        "scenePath": scene_path, "createdAt": now()});
    let requests = directory.join("requests");
    let responses = directory.join("responses");
    fs::create_dir_all(&responses)
        .await
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(&requests)
        .await
        .map_err(|e| e.to_string())?;
    // Dosya ancak tamamı yazıldıktan sonra kuyruğa görünür; yarım JSON okunmaz.
    let temporary = requests.join(format!("{id}.tmp"));
    fs::write(
        responses.join(format!("{id}.json")),
        serde_json::to_vec(
            &serde_json::json!({"id": id, "state": "queued", "phase": "Waiting for 3ds Max"}),
        )
        .unwrap(),
    )
    .await
    .map_err(|e| e.to_string())?;
    fs::write(
        &temporary,
        serde_json::to_vec(&request).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| e.to_string())?;
    fs::rename(&temporary, requests.join(format!("{id}.json")))
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn get_max_merge_status(
    instance_id: String,
    request_id: String,
) -> Result<serde_json::Value> {
    let instance = uuid_path(&bridge_root()?, &instance_id)?;
    let request_id = Uuid::parse_str(&request_id)
        .map_err(|_| "Invalid request identifier.")?
        .to_string();
    let response = read_json(
        &instance
            .join("responses")
            .join(format!("{request_id}.json")),
    )
    .await?;
    if response["id"] != request_id {
        return Err("Unexpected bridge response.".into());
    }
    if matches!(response["state"].as_str(), Some("queued" | "running")) {
        #[cfg(windows)]
        if let Ok(heartbeat) = read_json(&instance.join("heartbeat.json")).await {
            if let Some(pid) = heartbeat["pid"]
                .as_u64()
                .filter(|pid| *pid <= u32::MAX as u64)
            {
                if !process_alive(pid as u32) {
                    return Ok(
                        serde_json::json!({"id": request_id, "state": "error", "phase":
                        "3ds Max closed before the merge completed. Project copies have been kept. Restart Max and try again."}),
                    );
                }
            }
        }
        if response["state"] == "queued" {
            let path = instance
                .join("responses")
                .join(format!("{request_id}.json"));
            if fs::metadata(path)
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 60)
            {
                return Ok(
                    serde_json::json!({"id": request_id, "state": "error", "phase":
                    "3ds Max did not accept the request within 60 seconds. No merge was started. Check the plugin connection and try again."}),
                );
            }
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_zip_and_rar_can_enter_the_merge_queue() {
        for source in ["A:/chair.max", "A:/chair.ZIP", "A:/chair.rar", "A:/chair.RAR"] {
            assert!(supports_merge_source(Path::new(source)), "{source}");
        }
        for source in ["A:/chair.jpg", "A:/script.exe", "A:/folder.rar/model.pdf"] {
            assert!(!supports_merge_source(Path::new(source)), "{source}");
        }
    }

    #[test]
    fn old_plugins_do_not_claim_rar_support_and_new_plugins_advertise_it() {
        let heartbeat = serde_json::json!({"id":"test","version":1,"pid":1,"maxVersion":"28000",
            "scenePath":"main.max","projectPath":"project","busy":false,"updatedAt":100.0});
        let old: MaxInstance = serde_json::from_value(heartbeat.clone()).unwrap();
        assert_eq!(old.merge_extensions, vec!["max", "zip"]);
        let mut heartbeat = heartbeat;
        heartbeat["mergeExtensions"] = serde_json::json!(["max","zip","rar"]);
        let new: MaxInstance = serde_json::from_value(heartbeat).unwrap();
        assert!(new.merge_extensions.iter().any(|extension| extension == "rar"));
    }

    #[test]
    fn identifiers_cannot_escape_bridge_directory() {
        let root = Path::new("bridge");
        for id in ["../outside", "C:/outside", "", "not-a-uuid"] {
            assert!(uuid_path(root, id).is_err());
        }
        let id = Uuid::new_v4().to_string();
        assert_eq!(uuid_path(root, &id).unwrap(), root.join(id));
    }

    #[test]
    fn stale_or_incompatible_max_sessions_are_not_connected() {
        let mut instance = MaxInstance {
            id: Uuid::new_v4().to_string(),
            version: 1,
            pid: 123,
            max_version: "28000".into(),
            scene_path: String::new(),
            project_path: String::new(),
            busy: false,
            updated_at: 100.0,
            merge_extensions: legacy_merge_extensions(),
        };
        assert!(is_live(&instance, 110.0));
        assert!(!is_live(&instance, 116.0));
        assert!(!is_live(&instance, 90.0));
        instance.version = 2;
        assert!(!is_live(&instance, 110.0));
    }

    #[test]
    #[cfg(windows)]
    fn running_and_closed_processes_are_distinguished() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(u32::MAX));
    }
}
