type Result<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn reveal_in_explorer(file_path: String) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let norm_path = file_path.replace('/', "\\");
        let path = std::path::Path::new(&norm_path);

        if path.exists() {
            std::process::Command::new("explorer")
                .raw_arg(format!(r#"/select,"{}""#, norm_path))
                .spawn()
                .map_err(|e| format!("Failed to launch explorer: {}", e))?;
        } else if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().replace('/', "\\");
            std::process::Command::new("explorer")
                .raw_arg(format!(r#""{}""#, parent_str))
                .spawn()
                .map_err(|e| format!("Failed to launch explorer: {}", e))?;
        } else {
            std::process::Command::new("explorer")
                .raw_arg(format!(r#""{}""#, norm_path))
                .spawn()
                .map_err(|e| format!("Failed to launch explorer: {}", e))?;
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &file_path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        let dir = std::path::Path::new(&file_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(file_path);
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn validated_file_path(file_path: &str) -> Result<std::path::PathBuf> {
    if file_path.contains('\0') { return Err("Invalid file path".into()); }
    #[cfg(target_os = "windows")]
    let file_path = file_path.replace('/', "\\");
    // ShellExecute, canonicalize'ın eklediği uzun yol önekini her dosya ilişkilendirmesinde kabul etmez.
    #[cfg(target_os = "windows")]
    let file_path = file_path.strip_prefix("\\\\?\\UNC\\").map(|path|format!("\\\\{path}"))
        .unwrap_or_else(||file_path.strip_prefix("\\\\?\\").unwrap_or(&file_path).to_string());
    let path = std::path::PathBuf::from(file_path);
    if !path.is_absolute() || !path.is_file() {
        return Err("The file is unavailable or its path is not an absolute file path".into());
    }
    Ok(path)
}

#[tauri::command]
pub async fn open_with_default(file_path: String) -> Result<()> {
    // NAS erişimi async işçiyi durdurmaz; dosya adı hiçbir komut satırında yorumlanmaz.
    tokio::task::spawn_blocking(move || -> Result<()> {
        let path = validated_file_path(&file_path)?;
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::OsStrExt;
            #[link(name = "shell32")]
            extern "system" {
                fn ShellExecuteW(window: *mut std::ffi::c_void, operation: *const u16,
                    file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> isize;
            }
            let operation: Vec<u16> = std::ffi::OsStr::new("open").encode_wide().chain(Some(0)).collect();
            let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let result = unsafe { ShellExecuteW(std::ptr::null_mut(), operation.as_ptr(), file.as_ptr(),
                std::ptr::null(), std::ptr::null(), 1) };
            if result <= 32 {
                return Err(if result == 31 {
                    "No default application is associated with this file type. Choose an app using Windows Open with.".into()
                } else { format!("Windows could not open this file (error {result})") });
            }
        }
        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("open").arg(&path).status().map_err(|e| e.to_string())?;
            if !status.success() { return Err("The default application could not open this file".into()); }
        }
        #[cfg(target_os = "linux")]
        {
            let status = std::process::Command::new("xdg-open").arg(&path).status().map_err(|e| e.to_string())?;
            if !status.success() { return Err("The default application could not open this file".into()); }
        }
        Ok(())
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_open_rejects_relative_url_missing_directory_and_nul_paths() {
        for path in ["scene.max", "https://example.com/file.zip", "C:/missing-lumiadam-file.zip", "C:/file\0.zip"] {
            assert!(validated_file_path(path).is_err());
        }
        assert!(validated_file_path(std::env::temp_dir().to_str().unwrap()).is_err());
    }
    #[test]
    fn default_open_preserves_special_characters_as_a_file_path() {
        let folder = std::env::temp_dir().join(format!("lumiadam-open-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("kitchen & render %1 [9].zip");
        std::fs::write(&path,b"test").unwrap();
        assert_eq!(validated_file_path(path.to_str().unwrap()).unwrap(),path);
        #[cfg(windows)]
        assert_eq!(validated_file_path(path.canonicalize().unwrap().to_str().unwrap()).unwrap(),path);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
