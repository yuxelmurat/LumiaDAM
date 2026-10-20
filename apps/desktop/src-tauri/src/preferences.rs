//! Kurulum ve uygulama aynı yerel dil tercihini kullanır.
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn path() -> Result<PathBuf> {
    Ok(dirs::data_local_dir().ok_or_else(|| anyhow::anyhow!("Local application data is unavailable"))?
        .join("LumiaDAM/preferences.json"))
}
fn read(path: &Path) -> Result<Value> {
    match std::fs::read(path) {
        Ok(bytes) => {
            ensure!(bytes.len() <= 16384, "Preferences file is too large");
            let value: Value = serde_json::from_slice(&bytes)?;
            ensure!(value.is_object(), "Invalid preferences file");
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error.into()),
    }
}
fn save(path: &Path, language: &str) -> Result<()> {
    ensure!(matches!(language, "en" | "tr"), "Unsupported language");
    let mut value = read(path)?;
    value["language"] = language.into();
    std::fs::create_dir_all(path.parent().ok_or_else(|| anyhow::anyhow!("Invalid preferences path"))?)?;
    let temporary = path.with_file_name(format!("preferences-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(&value)?)?;
    let result = std::fs::rename(&temporary, path);
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result?;
    Ok(())
}
#[tauri::command]
pub fn get_app_language() -> Result<String, String> {
    (|| {
        let value = read(&path()?)?;
        Ok::<_, anyhow::Error>(if value["language"] == "tr" { "tr" } else { "en" }.to_owned())
    })().map_err(|error| error.to_string())
}
#[tauri::command]
pub fn set_app_language(language: String) -> Result<(), String> {
    path().and_then(|path| save(&path, &language)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_preserves_other_preferences_and_rejects_invalid_input() {
        let directory = std::env::temp_dir().join(format!("lumiadam-language-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("preferences.json");
        std::fs::write(&path, r#"{"other":42,"language":"en"}"#).unwrap();
        save(&path, "tr").unwrap();
        assert_eq!(read(&path).unwrap(), json!({"other":42,"language":"tr"}));
        assert!(save(&path, "de").is_err());
        std::fs::write(&path, "broken").unwrap();
        assert!(save(&path, "en").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
