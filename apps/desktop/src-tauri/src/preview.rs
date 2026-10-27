use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use image::imageops::FilterType;
use std::sync::Arc;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufReader};
use serde_json::json;
use tauri::{Emitter, Manager};
use crate::AppState;
use crate::db::resolve_path;

pub const THUMB_SIZE: u32 = 512;

/// Eski sürümde başarısız kalan HDR/EXR işlerini katalog başına yalnız bir kez yeniden dener.
pub(crate) async fn retry_failed_hdr_previews(pool: &sqlx::SqlitePool) -> Result<()> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(pool).await?;
    if version >= 1 { return Ok(()); }
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE preview_jobs SET status = 'queued' WHERE status = 'error' AND asset_id IN (SELECT id FROM assets WHERE extension IN ('hdr', 'exr'))"
    ).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE assets SET preview_status = 'pending' WHERE extension IN ('hdr', 'exr') AND id IN (SELECT asset_id FROM preview_jobs WHERE status = 'queued')"
    ).execute(&mut *tx).await?;
    sqlx::query("PRAGMA user_version = 1").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Rust'ın ek Python modülü olmadan önizleyebildiği görüntü formatları.
pub fn is_native_image(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "bmp" | "gif" | "tif" | "tiff" | "tga" | "webp" | "hdr" | "exr"
    )
}

/// Blazingly fast in-process image downsampling using the native Rust `image` crate.
/// Uses Triangle (bilinear) filtering for 4x–8x faster performance than Lanczos3.
pub fn generate_image_thumb(source: &Path, output: &Path) -> Result<()> {
    let img = image::open(source)?;

    if let Some(parent) = output.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let is_jpg = output
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("jpg") || s.eq_ignore_ascii_case("jpeg"))
        .unwrap_or(false);

    let is_hdr = source.extension().and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("hdr") || ext.eq_ignore_ascii_case("exr"));

    if is_hdr {
        // WebView HDR/EXR dosyasını doğrudan gösteremez; kayan nokta renkleri SDR'ye eşle.
        // Kırpmayı önlemek için ton eşlemesi kayan nokta görüntü küçültülmeden yapılır.
        let mut linear = match img {
            image::DynamicImage::ImageRgb32F(buffer) => buffer,
            other => other.to_rgb32f(),
        };
        for pixel in linear.pixels_mut() {
            for channel in &mut pixel.0 {
                let value = if channel.is_finite() { channel.max(0.0) } else { 0.0 };
                *channel = (value / (1.0 + value)).powf(1.0 / 2.2);
            }
        }
        let thumb = image::DynamicImage::ImageRgb32F(linear)
            .resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle).to_rgb32f();
        let mapped = image::RgbImage::from_fn(thumb.width(), thumb.height(), |x, y| {
            let [r, g, b] = thumb.get_pixel(x, y).0;
            image::Rgb([(r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8])
        });
        mapped.save_with_format(output, image::ImageFormat::Jpeg)?;
    } else {
        let thumb = img.resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle);
        if is_jpg {
            thumb.to_rgb8().save_with_format(output, image::ImageFormat::Jpeg)?;
        } else {
            thumb.save(output)?;
        }
    }
    Ok(())
}

/// Persistent Python worker process communicating over JSON-RPC on stdin/stdout.
/// Stays alive across requests to eliminate Python cold-start overhead (200-500ms per file).
struct PythonWorker {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl PythonWorker {
    async fn spawn(py_exec: &Path, worker_py: &Path) -> Result<Self> {
        if !worker_py.is_file() {
            return Err(anyhow!("Python preview worker is missing. Reinstall LumiaDAM or set OPENDAM_WORKER."));
        }
        let mut cmd = Command::new(py_exec);
        cmd.arg(worker_py)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONIOENCODING", "utf-8");

        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut child = cmd.spawn().map_err(|e| anyhow!(
            "Optional Python previews require Python 3.10+ and preview dependencies. Set OPENDAM_PYTHON to its executable. {e}"
        ))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("Failed to take python stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to take python stdout"))?;
        let reader = BufReader::new(stdout);

        Ok(Self { child, stdin, reader })
    }

    async fn execute(&mut self, req: &serde_json::Value) -> Result<serde_json::Value> {
        let req_str = req.to_string() + "\n";
        self.stdin.write_all(req_str.as_bytes()).await?;
        self.stdin.flush().await?;

        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Err(anyhow!("Python worker closed stdout unexpectedly"));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('{') {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    return Ok(val);
                }
            }
        }
    }

    async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}

pub fn start_preview_worker(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        // Dynamically resolve python worker and script paths
        let current_dir = std::env::current_dir().unwrap_or_default();
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| current_dir.clone());

        let mut search_roots = vec![current_dir.clone(), exe_dir.clone()];
        if let Ok(resources) = state.app_handle.path().resource_dir() {
            search_roots.insert(0, resources);
        }
        let mut curr = current_dir.clone();
        for _ in 0..4 {
            if let Some(parent) = curr.parent() {
                search_roots.push(parent.to_path_buf());
                curr = parent.to_path_buf();
            } else {
                break;
            }
        }
        let mut curr_exe = exe_dir.clone();
        for _ in 0..4 {
            if let Some(parent) = curr_exe.parent() {
                search_roots.push(parent.to_path_buf());
                curr_exe = parent.to_path_buf();
            } else {
                break;
            }
        }

        let py_exec = if let Ok(custom) = std::env::var("OPENDAM_PYTHON") {
            PathBuf::from(custom)
        } else {
            let mut found_py = None;
            for root in &search_roots {
                let candidates = [
                    root.join("packages").join("preview-engine").join("venv").join("Scripts").join("pythonw.exe"),
                    root.join("packages").join("preview-engine").join("venv").join("Scripts").join("python.exe"),
                    root.join("packages").join("preview-engine").join("venv").join("bin").join("python"),
                    root.join("venv").join("Scripts").join("pythonw.exe"),
                    root.join("venv").join("Scripts").join("python.exe"),
                    root.join("venv").join("bin").join("python"),
                ];
                if let Some(p) = candidates.into_iter().find(|p| p.exists()) {
                    found_py = Some(p);
                    break;
                }
            }
            found_py.unwrap_or_else(|| {
                let fallbacks = [PathBuf::from("pythonw"), PathBuf::from("python")];
                fallbacks.into_iter().find(|p| p.exists()).unwrap_or_else(|| PathBuf::from("python"))
            })
        };

        let worker_py = if let Ok(custom) = std::env::var("OPENDAM_WORKER") {
            PathBuf::from(custom)
        } else {
            let mut found_worker = None;
            for root in &search_roots {
                let candidates = [
                    root.join("preview-engine").join("worker.py"),
                    root.join("packages").join("preview-engine").join("worker.py"),
                    root.join("worker.py"),
                ];
                if let Some(p) = candidates.into_iter().find(|p| p.exists()) {
                    found_worker = Some(p);
                    break;
                }
            }
            found_worker.unwrap_or_else(|| PathBuf::from("worker.py"))
        };

        tracing::info!("Preview worker using Python: {:?}, Script: {:?}", py_exec, worker_py);

        let mut prepared_libraries = std::collections::HashSet::new();
        let mut py_worker: Option<PythonWorker> = None;

        loop {
            let libs = state.db.get_all_libraries().await;
            if libs.is_empty() {
                tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                continue;
            }

            let mut processed_any = false;

            for lib in libs {
                let pool = &lib.pool;
                if prepared_libraries.insert(lib.id.clone()) {
                    // Uygulama açıldıktan sonra eklenen katalogları da bir kez hazırla.
                    let _ = sqlx::query("UPDATE preview_jobs SET status = 'queued' WHERE status = 'processing'")
                        .execute(pool).await;
                    if let Err(error) = retry_failed_hdr_previews(pool).await {
                        tracing::error!("HDR/EXR preview retry migration failed: {error}");
                    }
                }
                let root_path = &lib.root_path;
                let thumb_dir = lib.catalog_dir.join("thumbs");
                let _ = tokio::fs::create_dir_all(&thumb_dir).await;

                // Poll batch of up to 10 jobs to minimize SQLite round-trip overhead
                let jobs: Vec<(String, String, String)> = sqlx::query_as(
                    "SELECT p.id, p.asset_id, a.file_path FROM preview_jobs p JOIN assets a ON p.asset_id = a.id WHERE p.status = 'queued' OR p.status = 'pending' LIMIT 10"
                ).fetch_all(pool).await.unwrap_or_default();

                for (job_id, asset_id, file_path) in jobs {
                    processed_any = true;

                    if let Err(e) = sqlx::query("UPDATE preview_jobs SET status = 'processing' WHERE id = ?")
                        .bind(&job_id)
                        .execute(pool)
                        .await
                    {
                        tracing::error!("Failed to update job status to processing: {}", e);
                        continue;
                    }

                    let full_source = if Path::new(&file_path).is_absolute() {
                        PathBuf::from(&file_path)
                    } else {
                        root_path.join(&file_path)
                    };

                    let file_stem = full_source
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("preview");

                    let ext = full_source
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    // Her varlığın önizleme dosyaları kendi klasöründe yer alır.
                    let asset_thumb_dir = thumb_dir.join(&asset_id);
                    let thumb_png = asset_thumb_dir.join(format!("{}_thumb.png", file_stem));
                    let thumb_jpg = asset_thumb_dir.join(format!("{}_thumb.jpg", file_stem));

                    let existing_thumb = if thumb_png.exists() {
                        Some(thumb_png.to_string_lossy().to_string())
                    } else if thumb_jpg.exists() {
                        Some(thumb_jpg.to_string_lossy().to_string())
                    } else {
                        None
                    };

                    if let Some(rel_thumb) = existing_thumb {
                        tracing::info!("Thumbnail already exists for {}, skipping generation", file_stem);
                        let _ = sqlx::query("UPDATE preview_jobs SET status = 'done' WHERE id = ?")
                            .bind(&job_id)
                            .execute(pool)
                            .await;
                        let _ = sqlx::query("UPDATE assets SET preview_status = 'done', thumbnail_path = ? WHERE id = ?")
                            .bind(&rel_thumb)
                            .bind(&asset_id)
                            .execute(pool)
                            .await;

                        let resolved_abs = resolve_path(root_path, &rel_thumb);
                        let _ = state.app_handle.emit("preview:done", json!({
                            "assetId": &asset_id,
                            "thumbnailPath": resolved_abs,
                        }));
                        continue;
                    }

                    // ── 1. Fast Native Rust Path for Standard Raster Images ──
                    if is_native_image(&ext) {
                        let thumb_file_name = if ext == "png" || ext == "webp" {
                            format!("{}_thumb.png", file_stem)
                        } else {
                            format!("{}_thumb.jpg", file_stem)
                        };
                        let thumb_dest = asset_thumb_dir.join(&thumb_file_name);
                        let rel_thumb = thumb_dest.to_string_lossy().to_string();

                        match generate_image_thumb(&full_source, &thumb_dest) {
                            Ok(()) => {
                                let _ = sqlx::query("UPDATE preview_jobs SET status = 'done' WHERE id = ?")
                                    .bind(&job_id)
                                    .execute(pool)
                                    .await;
                                let _ = sqlx::query("UPDATE assets SET preview_status = 'done', thumbnail_path = ? WHERE id = ?")
                                    .bind(&rel_thumb)
                                    .bind(&asset_id)
                                    .execute(pool)
                                    .await;

                                let resolved_abs = resolve_path(root_path, &rel_thumb);
                                let _ = state.app_handle.emit("preview:done", json!({
                                    "assetId": &asset_id,
                                    "thumbnailPath": resolved_abs,
                                }));
                                continue;
                            }
                            Err(e) => {
                                tracing::warn!("Native Rust image thumbnail failed for {}: {}. Falling back to Python.", full_source.display(), e);
                            }
                        }
                    }

                    // ── 2. Persistent Python Worker Path for 3D/CAD/DCC Formats ──
                    let mut worker_start_error = None;
                    if py_worker.is_none() {
                        match PythonWorker::spawn(&py_exec, &worker_py).await {
                            Ok(w) => py_worker = Some(w),
                            Err(e) => {
                                tracing::error!("Failed to spawn Python preview worker: {}", e);
                                worker_start_error = Some(e);
                            }
                        }
                    }

                    let req = json!({
                        "id": &job_id,
                        "method": "model_meta",
                        "params": {
                            "source": full_source.to_string_lossy().to_string(),
                            "output_dir": asset_thumb_dir.to_string_lossy().to_string()
                        }
                    });

                    let py_resp = if let Some(ref mut worker) = py_worker {
                        match tokio::time::timeout(std::time::Duration::from_secs(60), worker.execute(&req)).await {
                            Ok(Ok(val)) => Ok(val),
                            Ok(Err(e)) => {
                                tracing::error!("Python worker execute error: {}. Restarting worker.", e);
                                worker.kill().await;
                                py_worker = None;
                                Err(e)
                            }
                            Err(_) => {
                                tracing::error!("Python worker timed out after 60s processing {}. Killing worker.", file_stem);
                                worker.kill().await;
                                py_worker = None;
                                Err(anyhow!("Job timed out"))
                            }
                        }
                    } else {
                        Err(worker_start_error.unwrap_or_else(|| anyhow!("Python worker unavailable")))
                    };

                    let mut success = false;
                    let mut result_thumb = None;
                    let mut result_model = None;
                    let mut preview_error = py_resp.as_ref().err().map(|e| e.to_string())
                        .unwrap_or_else(|| "Preview worker did not return a usable preview".into());

                    if let Ok(resp) = py_resp {
                        if let Some(error) = resp.get("error").and_then(|value| value.as_str()) {
                            preview_error = error.to_string();
                        }
                        if let Some(res) = resp.get("result") {
                            if res.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                                success = true;
                                if let Some(thumb) = res.get("thumbnail").and_then(|v| v.as_str()) {
                                    result_thumb = Some(thumb.to_string());
                                }
                                if let Some(model) = res.get("preview_model").and_then(|v| v.as_str()) {
                                    result_model = Some(model.to_string());
                                }
                            } else {
                                preview_error = res.get("error").and_then(|value| value.as_str())
                                    .unwrap_or("Optional preview dependencies are missing or this format is unsupported").to_string();
                                tracing::error!("Python returned ok=false: {:?}", res);
                            }
                        } else {
                            tracing::error!("Python response missing result: {:?}", resp);
                        }
                    }

                    if success {
                        tracing::info!("Job {} succeeded, thumb: {:?}, model: {:?}", job_id, result_thumb, result_model);
                        let _ = sqlx::query("UPDATE preview_jobs SET status = 'done' WHERE id = ?")
                            .bind(&job_id)
                            .execute(pool)
                            .await;

                        let thumb_to_save_abs = result_thumb.clone().or_else(|| result_model.clone());
                        // Kaynak tüm sürücü olsa da önizleme yolu kaynak köküne göre kısaltılmaz.
                        let rel_thumb = thumb_to_save_abs;

                        if let Some(ref model_path) = result_model {
                            let _ = sqlx::query(
                                "UPDATE assets SET preview_status = 'done', thumbnail_path = ?, metadata = json_set(coalesce(nullif(metadata, ''), '{}'), '$.preview_model', ?) WHERE id = ?"
                            )
                            .bind(&rel_thumb)
                            .bind(model_path)
                            .bind(&asset_id)
                            .execute(pool)
                            .await;
                        } else if let Some(ref thumb_rel_val) = rel_thumb {
                            let _ = sqlx::query("UPDATE assets SET preview_status = 'done', thumbnail_path = ? WHERE id = ?")
                                .bind(thumb_rel_val)
                                .bind(&asset_id)
                                .execute(pool)
                                .await;
                        } else {
                            let _ = sqlx::query("UPDATE assets SET preview_status = 'done' WHERE id = ?")
                                .bind(&asset_id)
                                .execute(pool)
                                .await;
                        }

                        let resolved_display_thumb = rel_thumb.as_ref().map(|r| resolve_path(root_path, r));
                        let _ = state.app_handle.emit("preview:done", json!({
                            "assetId": &asset_id,
                            "thumbnailPath": &resolved_display_thumb,
                            "previewModel": &result_model,
                        }));
                    } else {
                        tracing::error!("Job {} failed", job_id);
                        let _ = sqlx::query("UPDATE preview_jobs SET status = 'error' WHERE id = ?")
                            .bind(&job_id)
                            .execute(pool)
                            .await;
                        let _ = sqlx::query("UPDATE assets SET preview_status = 'error' WHERE id = ?")
                            .bind(&asset_id)
                            .execute(pool)
                            .await;
                        let _ = sqlx::query("INSERT INTO settings(key, value) VALUES('preview_error', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                            .bind(&preview_error).execute(pool).await;
                        let _ = state.app_handle.emit("preview:error", json!({
                            "assetId": &asset_id,
                            "jobId": &job_id,
                        }));
                    }

                    // Yield execution between items to allow other async runtime tasks to progress
                    tokio::task::yield_now().await;
                }
            }

            if !processed_any {
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }
        }
    });
}

#[cfg(test)]
mod image_tests {
    use super::*;

    #[test]
    fn hdr_and_exr_generate_tone_mapped_jpeg_without_python() {
        let dir = std::env::temp_dir().join(format!("lumiadam-hdr-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pixels = image::ImageBuffer::from_pixel(2, 2, image::Rgb([4.0_f32, 1.0, 0.25]));
        let original = image::DynamicImage::ImageRgb32F(pixels);

        for (extension, format) in [("hdr", image::ImageFormat::Hdr), ("exr", image::ImageFormat::OpenExr)] {
            let source = dir.join(format!("sample.{extension}"));
            let output = dir.join(format!("{extension}_thumb.jpg"));
            original.save_with_format(&source, format).unwrap();
            assert!(is_native_image(extension));
            generate_image_thumb(&source, &output).unwrap();
            let result = image::open(&output).unwrap().to_rgb8();
            let [red, green, blue] = result.get_pixel(0, 0).0;
            assert!(red > green && green > blue && blue > 0, "{extension}: {red}, {green}, {blue}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
