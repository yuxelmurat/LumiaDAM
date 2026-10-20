use crate::db::resolve_path;
use crate::AppState;
use anyhow::{anyhow, Result};
use image::imageops::FilterType;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub const THUMB_SIZE: u32 = 512;
const MAX_CAD_PREVIEW_BYTES: u64 = 256 * 1024 * 1024;

pub async fn initialize(pool: &sqlx::SqlitePool) -> Result<()> {
    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(preview_jobs)")
            .fetch_all(pool)
            .await?;
    let mut tx = pool.begin().await?;
    for (name, definition) in [
        ("priority", "INTEGER NOT NULL DEFAULT 0"),
        ("lane", "TEXT NOT NULL DEFAULT 'auto'"),
    ] {
        if !columns.iter().any(|column| column.1 == name) {
            sqlx::query(&format!(
                "ALTER TABLE preview_jobs ADD COLUMN {name} {definition}"
            ))
            .execute(&mut *tx)
            .await?;
        }
    }
    sqlx::query(include_str!("../migrations/005_preview_queue.sql"))
        .execute(&mut *tx)
        .await?;
    // İki bekleyen durum birlikte sıralanırken tüm kuyruk için geçici tablo oluşmasın.
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_preview_ready ON preview_jobs(priority DESC,created_at,id,asset_id,lane) WHERE status IN ('queued','pending')")
        .execute(&mut *tx).await?;
    // Özet varsa büyük iş tablosunda tekrar aggregate sorgusu çalıştırılmaz.
    let initialized: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preview_summary WHERE id=1)")
            .fetch_one(&mut *tx)
            .await?;
    if !initialized {
        sqlx::query("INSERT INTO preview_summary(id,total,completed,pending,failed) SELECT 1,count(*),coalesce(sum(status='done'),0),coalesce(sum(status IN ('queued','pending','processing')),0),coalesce(sum(status='error'),0) FROM preview_jobs")
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Tarama ve görünür kart istekleri aynı format listesini kullanır.
pub fn is_previewable(ext: &str) -> bool {
    is_native_image(ext)
        || matches!(
            ext.to_ascii_lowercase().as_str(),
            "max"
                | "fbx"
                | "obj"
                | "3ds"
                | "stl"
                | "gltf"
                | "glb"
                | "dae"
                | "ply"
                | "skp"
                | "rfa"
                | "rvt"
                | "dwg"
                | "dxf"
                | "3dm"
                | "vrmat"
                | "psd"
                | "psb"
                | "ai" | "avif" | "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v"
                | "pdf" | "heic" | "heif" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "ico"
        )
}

/// Windows'ta uzantısı bilinmeyen dosya da görünür olduğunda kayıtlı sağlayıcıya sorulur.
pub fn can_request_preview(ext: &str) -> bool {
    is_previewable(ext) || cfg!(windows)
}

/// Eski bağlantı/codec hatalarını geliştirilmiş üreticiyle katalog başına bir kez yeniden dene.
pub(crate) async fn retry_failed_previews(pool: &sqlx::SqlitePool) -> Result<()> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(pool).await?;
    if version >= 2 { return Ok(()); }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE preview_jobs SET status='queued',lane='auto',error_message=NULL WHERE status='error'")
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE assets SET preview_status='pending' WHERE preview_status='error' AND id IN (SELECT asset_id FROM preview_jobs WHERE status='queued')")
        .execute(&mut *tx).await?;
    sqlx::query("PRAGMA user_version=2").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Rust'ın ek Python modülü olmadan önizleyebildiği görüntü formatları.
pub fn is_native_image(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "bmp" | "gif" | "tif" | "tiff" | "tga" | "webp" | "hdr" | "exr" | "svg"
    )
}

/// Görüntüyü native image bağımlılığı ve Triangle filtresiyle küçültür.
pub fn generate_image_thumb(source: &Path, output: &Path) -> Result<()> {
    if source.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("svg")) {
        // Vektör görsel küçültme gerektirmez; kart her açılışta NAS yerine yerel kopyayı okur.
        if let Some(parent) = output.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::copy(source, output)?;
        return Ok(());
    }
    let img = image::open(source)?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let is_jpg = output
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("jpg") || s.eq_ignore_ascii_case("jpeg"))
        .unwrap_or(false);

    let is_hdr = source
        .extension()
        .and_then(|s| s.to_str())
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
                let value = if channel.is_finite() {
                    channel.max(0.0)
                } else {
                    0.0
                };
                *channel = (value / (1.0 + value)).powf(1.0 / 2.2);
            }
        }
        let thumb = image::DynamicImage::ImageRgb32F(linear)
            .resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle)
            .to_rgb32f();
        let mapped = image::RgbImage::from_fn(thumb.width(), thumb.height(), |x, y| {
            let [r, g, b] = thumb.get_pixel(x, y).0;
            image::Rgb([
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
            ])
        });
        mapped.save_with_format(output, image::ImageFormat::Jpeg)?;
    } else {
        let thumb = img.resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle);
        if is_jpg {
            thumb
                .to_rgb8()
                .save_with_format(output, image::ImageFormat::Jpeg)?;
        } else {
            thumb.save(output)?;
        }
    }
    Ok(())
}

/// DWG/DXF içinde kayıtlı önizlemeyi çizimin tamamını ayrıştırmadan yerel önbelleğe çıkarır.
pub fn generate_cad_thumb(source: &Path, output: &Path) -> Result<()> {
    anyhow::ensure!(std::fs::metadata(source)?.len() <= MAX_CAD_PREVIEW_BYTES,
        "Drawing is too large for the embedded preview reader");
    let bytes = std::fs::read(source)?;
    let preview = exav_render::cad::preview(&bytes)
        .ok_or_else(|| anyhow!("Drawing contains no embedded PNG/BMP preview"))?;
    let format = match preview.format {
        exav_render::cad::PreviewFormat::Bmp => image::ImageFormat::Bmp,
        exav_render::cad::PreviewFormat::Png => image::ImageFormat::Png,
    };
    let image = image::load_from_memory_with_format(&preview.data, format)?;
    if let Some(parent) = output.parent() { std::fs::create_dir_all(parent)?; }
    image.thumbnail(THUMB_SIZE, THUMB_SIZE)
        .save_with_format(output, image::ImageFormat::Png)?;
    Ok(())
}

/// Python sürecini işler arasında açık tutarak başlatma maliyetini ortadan kaldırır.
struct PythonWorker {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl PythonWorker {
    async fn spawn(py_exec: &Path, worker_py: &Path) -> Result<Self> {
        let mut cmd = Command::new(py_exec);
        cmd.arg(worker_py)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .kill_on_drop(true);

        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to take python stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to take python stdout"))?;
        let reader = BufReader::new(stdout);

        Ok(Self {
            child,
            stdin,
            reader,
        })
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

fn python_paths() -> (PathBuf, PathBuf) {
    // Python yürütücüsü ve script yolunu mevcut kurulumdan çöz.
    let current_dir = std::env::current_dir().unwrap_or_default();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| current_dir.clone());

    let mut search_roots = vec![current_dir.clone(), exe_dir.clone()];
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
                root.join("packages")
                    .join("preview-engine")
                    .join("venv")
                    .join("Scripts")
                    .join("pythonw.exe"),
                root.join("packages")
                    .join("preview-engine")
                    .join("venv")
                    .join("Scripts")
                    .join("python.exe"),
                root.join("packages")
                    .join("preview-engine")
                    .join("venv")
                    .join("bin")
                    .join("python"),
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
            fallbacks
                .into_iter()
                .find(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from("python"))
        })
    };

    let worker_py = if let Ok(custom) = std::env::var("OPENDAM_WORKER") {
        PathBuf::from(custom)
    } else {
        let mut found_worker = None;
        for root in &search_roots {
            let candidates = [
                root.join("packages")
                    .join("preview-engine")
                    .join("worker.py"),
                root.join("worker.py"),
            ];
            if let Some(p) = candidates.into_iter().find(|p| p.exists()) {
                found_worker = Some(p);
                break;
            }
        }
        found_worker.unwrap_or_else(|| PathBuf::from("worker.py"))
    };

    tracing::info!(
        "Preview worker using Python: {:?}, Script: {:?}",
        py_exec,
        worker_py
    );

    (py_exec, worker_py)
}

const NATIVE_EXTENSIONS: &str =
    "'jpg','jpeg','png','bmp','gif','tif','tiff','tga','webp','hdr','exr','svg','dwg','dxf'";
const NATIVE_CONCURRENCY: usize = 2;

#[derive(Debug, sqlx::FromRow)]
struct ClaimedJob {
    id: String,
    asset_id: String,
    file_path: String,
    extension: String,
    modified_at: i64,
    size_bytes: i64,
}

/// Tek SQLite yazısı hem seçer hem sahiplenir; iki şerit aynı işi alamaz.
async fn claim_job(pool: &sqlx::SqlitePool, native: bool) -> Result<Option<ClaimedJob>> {
    claim_ready_job(pool,native,None).await
}

async fn claim_ready_job(pool: &sqlx::SqlitePool, native: bool, local_prefix: Option<&str>) -> Result<Option<ClaimedJob>> {
    // Boş kuyrukta yazma kilidi alan sahiplenme sorgusunu çalıştırma.
    let pending: i64 = sqlx::query_scalar("SELECT pending FROM preview_summary WHERE id=1").fetch_one(pool).await?;
    if pending == 0 { return Ok(None); }
    let predicate = if native {
        format!("p.lane <> 'python' AND lower(a.extension) IN ({NATIVE_EXTENSIONS})")
    } else {
        format!("(p.lane = 'python' OR lower(a.extension) NOT IN ({NATIVE_EXTENSIONS}))")
    };
    let local_clause=if local_prefix.is_some() { "AND (p.error_message IS NULL OR p.error_message <> 'Local copy unavailable') AND p.asset_id IN (SELECT o.asset_id FROM offline_assets o JOIN assets cached ON cached.id=o.asset_id WHERE o.size_bytes=cached.size_bytes AND o.modified_at=cached.modified_at UNION SELECT id FROM assets WHERE replace(file_path,char(92),'/')>=? AND replace(file_path,char(92),'/')<?)" } else { "" };
    // NAS beklerken küçük yerel aday listesiyle, diğer durumda sıralı kuyrukla başla.
    let queue_index=if local_prefix.is_some() {"idx_preview_asset"} else {"idx_preview_ready"};
    let sql = format!("UPDATE preview_jobs SET status='processing' WHERE id=(
        SELECT p.id FROM preview_jobs p INDEXED BY {queue_index} JOIN assets a ON a.id=p.asset_id
        WHERE p.status IN ('queued','pending') AND {predicate} {local_clause}
        AND NOT EXISTS (SELECT 1 FROM preview_jobs active WHERE active.asset_id=p.asset_id AND active.status='processing')
        ORDER BY p.priority DESC,p.created_at,p.id LIMIT 1)
        AND status IN ('queued','pending') RETURNING id,asset_id,
        (SELECT file_path FROM assets WHERE assets.id=preview_jobs.asset_id) AS file_path,
        (SELECT extension FROM assets WHERE assets.id=preview_jobs.asset_id) AS extension,
        (SELECT modified_at FROM assets WHERE assets.id=preview_jobs.asset_id) AS modified_at,
        (SELECT size_bytes FROM assets WHERE assets.id=preview_jobs.asset_id) AS size_bytes");
    let mut query=sqlx::query_as(&sql);
    if let Some(prefix)=local_prefix {query=query.bind(prefix).bind(format!("{}0",prefix.trim_end_matches('/')));}
    Ok(query.fetch_optional(pool).await?)
}

async fn generate_image_thumb_async(
    source: PathBuf,
    output: PathBuf,
    slots: Arc<tokio::sync::Semaphore>,
) -> Result<()> {
    // Zaman aşımında çalışan iş kotasını korur; kopmuş NAS sınırsız iş parçacığı oluşturmaz.
    tokio::time::timeout(std::time::Duration::from_secs(60), async move {
        let permit = slots.acquire_owned().await?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            generate_image_thumb(&source, &output)
        }).await?
    }).await??;
    Ok(())
}

/// Yalnız üretim sırasında çağrılır; katalogda gezinmek kaynak diske dokunmaz.
async fn source_available(source: PathBuf, root: PathBuf, slots: Arc<tokio::sync::Semaphore>) -> bool {
    // Birden çok kökü olan eski kütüphanelerde ikinci sürücüyü ilk köke göre değerlendirme.
    let root = if crate::storage::path_is_within(&source, &root) { root }
        else { source.ancestors().last().unwrap_or(&source).to_path_buf() };
    tokio::time::timeout(std::time::Duration::from_secs(8), async move {
        let Ok(permit) = slots.acquire_owned().await else { return false; };
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            match std::fs::metadata(&source) {
                Ok(_) => true,
                // Silinmiş tek dosya diğer dosyaların kuyruğunu bekletmez.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound =>
                    std::fs::read_dir(root).is_ok(),
                Err(_) => false,
            }
        }).await.unwrap_or(false)
    }).await.unwrap_or(false)
}

/// Bağlantı hatası hazır küçük resmi veya varlığın durumunu bozmaz.
async fn defer_unavailable(pool: &sqlx::SqlitePool, job: &ClaimedJob) -> Result<()> {
    sqlx::query("UPDATE preview_jobs SET status='queued',error_message='Source unavailable; waiting to retry' WHERE id=? AND status='processing'")
        .bind(&job.id).execute(pool).await?;
    Ok(())
}

async fn pause_unavailable(lib: &crate::db::LibraryHandle, job: &ClaimedJob) {
    lib.preview_retry_at.store(chrono::Utc::now().timestamp_millis() + 60_000, std::sync::atomic::Ordering::Relaxed);
    if let Err(error) = defer_unavailable(&lib.pool, job).await {
        tracing::error!("Cannot defer unavailable preview: {error}");
    }
}

/// Native hata işi ağır şeride bırakır; diğer görüntüler çalışmayı sürdürür.
async fn defer_to_python(pool: &sqlx::SqlitePool, job: &ClaimedJob) -> Result<()> {
    sqlx::query(
        "UPDATE preview_jobs SET status='queued',lane='python' WHERE id=? AND status='processing'",
    )
    .bind(&job.id)
    .execute(pool)
    .await?;
    Ok(())
}

/// İş ve varlık birlikte tamamlanır; taramanın yeni dosya sürümü eski sonuçla ezilmez.
async fn complete_job(
    pool: &sqlx::SqlitePool,
    job: &ClaimedJob,
    success: bool,
    thumb: Option<&str>,
    model: Option<&str>,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE preview_jobs SET status=? WHERE id=? AND status='processing' AND EXISTS (SELECT 1 FROM assets WHERE id=? AND modified_at=? AND size_bytes=?)")
        .bind(if success { "done" } else { "error" }).bind(&job.id).bind(&job.asset_id)
        .bind(job.modified_at).bind(job.size_bytes).execute(&mut *tx).await?.rows_affected();
    if changed == 0 {
        // Watcher devre dışıyken rename gibi bir yazı eski işi silemeyebilir; kilitli kalmasın.
        sqlx::query("UPDATE preview_jobs SET status='queued',lane='auto' WHERE id=? AND status='processing'")
            .bind(&job.id).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(false);
    }
    sqlx::query("UPDATE assets SET preview_status=?, thumbnail_path=CASE WHEN ? THEN ? ELSE thumbnail_path END,
        metadata=CASE WHEN ? IS NOT NULL THEN json_set(coalesce(nullif(metadata,''),'{}'),'$.preview_model',?) ELSE metadata END
        WHERE id=? AND modified_at=? AND size_bytes=?")
        .bind(if success { "done" } else { "error" }).bind(success).bind(thumb.or(model))
        .bind(model).bind(model).bind(&job.asset_id).bind(job.modified_at).bind(job.size_bytes)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

fn output_dir(lib: &crate::db::LibraryHandle, job: &ClaimedJob) -> PathBuf {
    // İş kimliği tarama sırasında değiştiği için eski çalışan iş yeni küçük resmi ezemez.
    lib.catalog_dir
        .join("thumbs")
        .join(&job.asset_id)
        .join(format!("{}-{}-{}", job.modified_at, job.size_bytes, job.id))
}

/// Kurulu Windows codec/thumbnail sağlayıcısı kullanılır; takılan sağlayıcı sonlandırılır.
async fn run_shell(source: &Path, output: &Path) -> Result<(Option<String>, Option<String>)> {
    #[cfg(windows)]
    {
        let thumbnail = output.join("windows_thumb.png");
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("--lumiadam-shell-thumbnail").arg(source).arg(&thumbnail)
            .creation_flags(0x08000000).kill_on_drop(true);
        let result = tokio::time::timeout(std::time::Duration::from_secs(20), command.output()).await??;
        anyhow::ensure!(result.status.success(), "Windows thumbnail: {}", String::from_utf8_lossy(&result.stderr));
        anyhow::ensure!(thumbnail.is_file(), "Windows thumbnail provider returned no image");
        Ok((Some(thumbnail.to_string_lossy().into_owned()), None))
    }
    #[cfg(not(windows))]
    { let _ = (source, output); Err(anyhow!("Windows thumbnails are unavailable on this platform")) }
}

async fn run_python(
    worker: &mut Option<PythonWorker>,
    py_exec: &Path,
    worker_py: &Path,
    job: &ClaimedJob,
    source: &Path,
    output: &Path,
) -> Result<(Option<String>, Option<String>)> {
    if worker.is_none() {
        *worker = Some(PythonWorker::spawn(py_exec, worker_py).await?);
    }
    let image_fallback = (is_native_image(&job.extension) || job.extension.eq_ignore_ascii_case("avif"))
        && !matches!(job.extension.to_ascii_lowercase().as_str(), "hdr" | "exr");
    let video = matches!(job.extension.to_ascii_lowercase().as_str(), "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v");
    let req = if video {
        json!({"id":job.id,"method":"video_thumb","params":{
            "source":source.to_string_lossy(),"output":output.join("video_thumb.jpg").to_string_lossy(),"timestamp":"00:00:00"}})
    } else if image_fallback {
        // PNG çıktısı saydam görüntülerde Pillow'nun JPEG renk modu hatasını önler.
        json!({"id":job.id,"method":"image_thumb","params":{
            "source":source.to_string_lossy(),"output":output.join("image_thumb.png").to_string_lossy(),"size":THUMB_SIZE}})
    } else {
        json!({"id":job.id,"method":"model_meta","params":{
            "source":source.to_string_lossy(),"output_dir":output.to_string_lossy()}})
    };
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        worker.as_mut().unwrap().execute(&req),
    )
    .await;
    let reply = match reply {
        Ok(Ok(reply)) => reply,
        error => {
            if let Some(worker) = worker.as_mut() {
                worker.kill().await;
            }
            *worker = None;
            return Err(anyhow!("Python preview failed or timed out: {error:?}"));
        }
    };
    let result = reply
        .get("result")
        .ok_or_else(|| anyhow!("Python response missing result"))?;
    anyhow::ensure!(
        result.get("ok").and_then(|value| value.as_bool()) == Some(true),
        "Python preview failed: {result}"
    );
    Ok((
        result
            .get("thumbnail")
            .or_else(|| result.get("output"))
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        result
            .get("preview_model")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
    ))
}

pub fn start_preview_worker(state: Arc<AppState>) {
    let prepared = Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new()));
    let slots = Arc::new(tokio::sync::Semaphore::new(NATIVE_CONCURRENCY));
    let probe_slots = Arc::new(tokio::sync::Semaphore::new(2));
    let (py_exec, worker_py) = python_paths();
    // İki native şerit ve tek persistent Python şeridi birbirini beklemez.
    for lane in 0..=NATIVE_CONCURRENCY {
        let native = lane < NATIVE_CONCURRENCY;
        let state = state.clone();
        let prepared = prepared.clone();
        let slots = slots.clone();
        let probe_slots = probe_slots.clone();
        let py_exec = py_exec.clone();
        let worker_py = worker_py.clone();
        tauri::async_runtime::spawn(async move {
            let mut worker = None;
            let mut last_library = None;
            loop {
                // Yeni işler bekleme aralığını keser; kaçırılan sinyal için seyrek fallback bulunur.
                let wakeup = state.db.preview_wakeup.notified();
                tokio::pin!(wakeup);
                wakeup.as_mut().enable();
                let mut libs = state.db.get_all_libraries().await;
                libs.sort_by(|left, right| left.id.cmp(&right.id));
                // Her işten sonra sıradaki kütüphaneye geç; büyük katalog diğerlerini aç bırakmaz.
                if let Some(ref last) = last_library {
                    if let Some(position) = libs.iter().position(|lib| &lib.id == last) {
                        let len = libs.len();
                        libs.rotate_left((position + 1) % len);
                    }
                }
                let mut processed = false;
                for lib in libs {
                    {
                        let mut prepared = prepared.lock().await;
                        if !prepared.contains(&lib.id) {
                            let recover = sqlx::query(
                                "UPDATE preview_jobs SET status='queued' WHERE status='processing'",
                            )
                            .execute(&lib.pool)
                            .await;
                            if let Err(error) = recover {
                                tracing::error!("Preview recovery failed: {error}");
                                continue;
                            }
                            if let Err(error) = retry_failed_previews(&lib.pool).await {
                                tracing::error!("Preview retry migration failed: {error}");
                                continue;
                            }
                            prepared.insert(lib.id.clone());
                        }
                    }
                    let paused=lib.preview_retry_at.load(std::sync::atomic::Ordering::Relaxed) > chrono::Utc::now().timestamp_millis();
                    let local_prefix=format!("{}/",lib.catalog_dir.to_string_lossy().replace('\\',"/"));
                    let claimed=if paused {claim_ready_job(&lib.pool,native,Some(&local_prefix)).await} else {claim_job(&lib.pool,native).await};
                    let job = match claimed {
                        Ok(Some(job)) => job,
                        Ok(None) => continue,
                        Err(error) => {
                            tracing::error!("Preview claim failed: {error}");
                            continue;
                        }
                    };
                    last_library = Some(lib.id.clone());
                    processed = true;
                    let original=PathBuf::from(resolve_path(&lib.root_path, &job.file_path));
                    let cached=crate::workflow::cached_source(&lib,&job.asset_id,job.size_bytes,job.modified_at).await.unwrap_or_else(|error| {tracing::warn!("Cached preview source unavailable: {error}");None});
                    let source=cached.unwrap_or(original);
                    let local=crate::storage::path_is_within(&source,&lib.catalog_dir);
                    if paused && !local {
                        let _=sqlx::query("UPDATE preview_jobs SET status='queued',error_message='Local copy unavailable' WHERE id=? AND status='processing'").bind(&job.id).execute(&lib.pool).await;
                        break;
                    }
                    let output = output_dir(&lib, &job);
                    let started = std::time::Instant::now();
                    if !source_available(source.clone(), lib.root_path.clone(), probe_slots.clone()).await {
                        pause_unavailable(&lib, &job).await;
                        let _ = state.app_handle.emit("library:activity", json!({"libraryId":lib.id}));
                        break;
                    }
                    if !local {lib.preview_retry_at.store(0, std::sync::atomic::Ordering::Relaxed);}
                    // Büyük HDR ortam haritalarında önce Windows'un küçük resim yolunu kullan.
                    let hdr_shell = if native && cfg!(windows) && matches!(job.extension.to_ascii_lowercase().as_str(), "hdr" | "exr") {
                        run_shell(&source, &output).await.ok()
                    } else { None };
                    let result = if let Some(result) = hdr_shell {
                        Ok(result)
                    } else if native {
                        if matches!(job.extension.to_ascii_lowercase().as_str(), "dwg" | "dxf") {
                            let thumb = output.join("cad_thumb.png");
                            let cad_source = source.clone();
                            let cad_thumb = thumb.clone();
                            let result = tokio::task::spawn_blocking(move ||
                                generate_cad_thumb(&cad_source, &cad_thumb)).await;
                            match result {
                                Ok(Ok(())) => Ok((Some(thumb.to_string_lossy().into_owned()), None)),
                                error => {
                                    tracing::warn!("Embedded CAD preview unavailable, using Windows/Python fallback: {error:?}");
                                    if let Err(error) = defer_to_python(&lib.pool, &job).await {
                                        tracing::error!("CAD preview fallback enqueue failed: {error}");
                                    }
                                    state.db.preview_wakeup.notify_waiters();
                                    break;
                                }
                            }
                        } else {
                        let stem = source
                            .file_stem()
                            .and_then(|value| value.to_str())
                            .unwrap_or("preview");
                        let format = if matches!(
                            job.extension.to_ascii_lowercase().as_str(),
                            "png" | "webp"
                        ) {
                            "png"
                        } else {
                            "jpg"
                        };
                        let format = if job.extension.eq_ignore_ascii_case("svg") { "svg" } else { format };
                        let thumb = output.join(format!("{stem}_thumb.{format}"));
                        match generate_image_thumb_async(
                            source.clone(),
                            thumb.clone(),
                            slots.clone(),
                        )
                        .await
                        {
                            Ok(()) => Ok((Some(thumb.to_string_lossy().into_owned()), None)),
                            Err(error) => {
                                tracing::warn!("Native preview failed, using Windows/Python fallback: {error}");
                                if let Err(error) = defer_to_python(&lib.pool, &job).await {
                                    tracing::error!("Preview fallback enqueue failed: {error}");
                                }
                                state.db.preview_wakeup.notify_waiters();
                                break;
                            }
                        }
                        }
                    } else {
                        match run_shell(&source, &output).await {
                            Ok(result) => Ok(result),
                            Err(shell_error) => run_python(&mut worker, &py_exec, &worker_py, &job, &source, &output)
                                .await.map_err(|error| anyhow!("{shell_error:#}; {error:#}")),
                        }
                    };
                    let (success, thumb, model) = match result {
                        Ok((thumb, model)) => (true, thumb, model),
                        Err(error) => {
                            if !source_available(source.clone(), lib.root_path.clone(), probe_slots.clone()).await {
                                pause_unavailable(&lib, &job).await;
                                let _ = state.app_handle.emit("library:activity", json!({"libraryId":lib.id}));
                                break;
                            }
                            tracing::error!("Preview failed for {}: {error}", source.display());
                            let _ = sqlx::query("UPDATE preview_jobs SET error_message=? WHERE id=?")
                                .bind(format!("{error:#}")).bind(&job.id).execute(&lib.pool).await;
                            (false, None, None)
                        }
                    };
                    match complete_job(&lib.pool, &job, success, thumb.as_deref(), model.as_deref())
                        .await
                    {
                        Ok(true) => {
                            let event = if success {
                                "preview:done"
                            } else {
                                "preview:error"
                            };
                            let _ = state.app_handle.emit(event,json!({"assetId":job.asset_id,"jobId":job.id,
                                "thumbnailPath":thumb.or_else(||model.clone()),"previewModel":model}));
                        }
                        Ok(false) => {}
                        Err(error) => {
                            tracing::error!("Preview completion transaction failed: {error}");
                            let _ = sqlx::query("UPDATE preview_jobs SET status='queued' WHERE id=? AND status='processing'").bind(&job.id).execute(&lib.pool).await;
                        }
                    }
                    tracing::debug!(asset_id=%job.asset_id,native,elapsed_ms=started.elapsed().as_millis(),"Preview job finished");
                    break;
                }
                if !processed {
                    tokio::select! {
                        _ = &mut wakeup => {},
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {},
                    }
                } else {
                    tokio::task::yield_now().await;
                }
            }
        });
    }
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

        for (extension, format) in [
            ("hdr", image::ImageFormat::Hdr),
            ("exr", image::ImageFormat::OpenExr),
        ] {
            let source = dir.join(format!("sample.{extension}"));
            let output = dir.join(format!("{extension}_thumb.jpg"));
            original.save_with_format(&source, format).unwrap();
            assert!(is_native_image(extension));
            generate_image_thumb(&source, &output).unwrap();
            let result = image::open(&output).unwrap().to_rgb8();
            let [red, green, blue] = result.get_pixel(0, 0).0;
            assert!(
                red > green && green > blue && blue > 0,
                "{extension}: {red}, {green}, {blue}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn svg_preview_is_a_local_vector_copy() {
        let dir = std::env::temp_dir().join(format!("lumiadam-svg-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.svg");
        let output = dir.join("cache/preview.svg");
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="cyan"/></svg>"#;
        std::fs::write(&source,svg).unwrap();
        generate_image_thumb(&source,&output).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(),svg);
        std::fs::remove_file(&source).unwrap();
        assert!(output.is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[tokio::test]
    async fn paused_library_claims_local_copies_and_exports_without_claiming_nas_or_stale_copies() {
        let pool=pool().await;
        sqlx::query("CREATE TABLE offline_assets(asset_id TEXT PRIMARY KEY,file_name TEXT,size_bytes INTEGER,modified_at INTEGER)").execute(&pool).await.unwrap();
        for id in ["nas","cached","stale","export","outside"] {asset(&pool,id,"jpg").await;job(&pool,&format!("{id}-job"),id,"queued",if id=="nas" {10000} else {0}).await;}
        sqlx::query("INSERT INTO offline_assets VALUES('cached','copy.jpg',12,1),('stale','copy.jpg',13,1); UPDATE assets SET file_path='C:/local-catalog/exports/render.jpg' WHERE id='export'; UPDATE assets SET file_path='C:/local-catalog-other/render.jpg' WHERE id='outside';").execute(&pool).await.unwrap();
        assert_eq!(claim_ready_job(&pool,true,Some("C:/local-catalog/")).await.unwrap().unwrap().asset_id,"cached");
        assert_eq!(claim_ready_job(&pool,true,Some("C:/local-catalog/")).await.unwrap().unwrap().asset_id,"export");
        assert!(claim_ready_job(&pool,true,Some("C:/local-catalog/")).await.unwrap().is_none());
        assert_eq!(sqlx::query_scalar::<_,String>("SELECT status FROM preview_jobs WHERE asset_id='nas'").fetch_one(&pool).await.unwrap(),"queued");
        pool.close().await;
    }

    #[tokio::test]
    async fn offline_job_remains_pending_and_cached_image_survives() {
        let pool = pool().await;
        asset(&pool, "offline", "jpg").await;
        job(&pool, "offline-job", "offline", "queued", 0).await;
        sqlx::query("UPDATE assets SET thumbnail_path='local/cache.jpg',preview_status='done'").execute(&pool).await.unwrap();
        let claimed = claim_job(&pool, true).await.unwrap().unwrap();
        defer_unavailable(&pool, &claimed).await.unwrap();
        assert_eq!(counts(&pool).await, (1, 0, 1, 0));
        let row: (String, String) = sqlx::query_as("SELECT thumbnail_path,preview_status FROM assets").fetch_one(&pool).await.unwrap();
        assert_eq!(row, ("local/cache.jpg".into(), "done".into()));
        assert!(claim_job(&pool, true).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn source_probe_distinguishes_missing_file_from_disconnected_folder() {
        let dir = std::env::temp_dir().join(format!("lumiadam-offline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        // Silinmiş tek dosya kuyruğun tümünü bekletmez; kopmuş kök ise bekletir.
        assert!(source_available(dir.join("missing.jpg"), dir.clone(), slots.clone()).await);
        assert!(source_available(dir.join("deleted-folder/missing.jpg"), dir.clone(), slots.clone()).await);
        std::fs::remove_dir(&dir).unwrap();
        assert!(!source_available(dir.join("missing.jpg"), dir.clone(), slots).await);
    }

    async fn base_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../migrations/002_library_schema.sql"))
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    async fn pool() -> sqlx::SqlitePool {
        let pool = base_pool().await;
        initialize(&pool).await.unwrap();
        pool
    }

    async fn asset(pool: &sqlx::SqlitePool, id: &str, ext: &str) {
        sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES(?,?,?,?,'image',12,1,1)")
            .bind(id).bind(format!("{id}.{ext}")).bind(id).bind(ext).execute(pool).await.unwrap();
    }

    async fn job(pool: &sqlx::SqlitePool, id: &str, asset_id: &str, status: &str, priority: i64) {
        sqlx::query(
            "INSERT INTO preview_jobs(id,asset_id,status,created_at,priority) VALUES(?,?,?,1,?)",
        )
        .bind(id)
        .bind(asset_id)
        .bind(status)
        .bind(priority)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn counts(pool: &sqlx::SqlitePool) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT total,completed,pending,failed FROM preview_summary WHERE id=1")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn summary_tracks_insert_transition_delete() {
        let pool = pool().await;
        asset(&pool, "asset", "jpg").await;
        for (id, status) in [
            ("a", "queued"),
            ("b", "pending"),
            ("c", "processing"),
            ("d", "done"),
            ("e", "error"),
        ] {
            job(&pool, id, "asset", status, 0).await;
        }
        assert_eq!(counts(&pool).await, (5, 1, 3, 1));
        sqlx::query("UPDATE preview_jobs SET status='done' WHERE id='a'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE preview_jobs SET status='error' WHERE id='b'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE preview_jobs SET priority=9 WHERE id='c'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(counts(&pool).await, (5, 2, 1, 2));
        sqlx::query("DELETE FROM preview_jobs WHERE id IN ('c','d')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(counts(&pool).await, (3, 1, 0, 2));
        initialize(&pool).await.unwrap();
        assert_eq!(counts(&pool).await, (3, 1, 0, 2));
    }

    #[tokio::test]
    async fn migration_aggregates_old_jobs_once() {
        let pool = base_pool().await;
        asset(&pool, "old", "jpg").await;
        sqlx::query("INSERT INTO preview_jobs(id,asset_id,status,created_at) VALUES('old','old','processing',1)")
            .execute(&pool).await.unwrap();
        initialize(&pool).await.unwrap();
        assert_eq!(counts(&pool).await, (1, 0, 1, 0));
        sqlx::query("UPDATE preview_jobs SET status='error'")
            .execute(&pool)
            .await
            .unwrap();
        initialize(&pool).await.unwrap();
        assert_eq!(counts(&pool).await, (1, 0, 0, 1));
    }

    #[tokio::test]
    async fn requests_preserve_jobs_and_allow_windows_provider_for_unknown_extensions() {
        let pool = pool().await;
        for (id, ext) in [
            ("active", "jpg"),
            ("done", "max"),
            ("new", "png"),
            ("text", "txt"),
            ("failed", "jpg"),
        ] {
            asset(&pool, id, ext).await;
        }
        job(&pool, "active-job", "active", "processing", 0).await;
        job(&pool, "done-job", "done", "done", 0).await;
        job(&pool, "failed-job", "failed", "error", 0).await;
        sqlx::query("UPDATE assets SET preview_status='done' WHERE id='done'")
            .execute(&pool)
            .await
            .unwrap();
        let ids = ["active", "done", "new", "text", "failed", "unknown"].map(str::to_owned);
        crate::commands::preview::enqueue_previews(&pool, &ids, false)
            .await
            .unwrap();
        let shell_candidate = i64::from(cfg!(windows));
        assert_eq!(counts(&pool).await, (4 + shell_candidate, 1, 2 + shell_candidate, 1));
        crate::commands::preview::enqueue_previews(&pool, &ids, true)
            .await
            .unwrap();
        crate::commands::preview::enqueue_previews(&pool, &ids, true)
            .await
            .unwrap();
        assert_eq!(counts(&pool).await, (4 + shell_candidate, 1, 3 + shell_candidate, 0));
        let rows: Vec<(String,String)> = sqlx::query_as("SELECT id,status FROM preview_jobs WHERE asset_id IN ('active','done') ORDER BY asset_id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(
            rows,
            vec![
                ("active-job".into(), "processing".into()),
                ("done-job".into(), "done".into())
            ]
        );
    }

    #[tokio::test]
    async fn visible_priority_claims_atomically_and_native_bypasses_heavy_queue() {
        let pool = pool().await;
        for (id, ext) in [
            ("heavy", "max"),
            ("old", "jpg"),
            ("visible", "jpg"),
            ("second", "png"),
        ] {
            asset(&pool, id, ext).await;
        }
        job(&pool, "heavy-job", "heavy", "queued", 900).await;
        job(&pool, "old-job", "old", "queued", 0).await;
        job(&pool, "visible-job", "visible", "queued", 0).await;
        job(&pool, "second-job", "second", "queued", 0).await;
        crate::commands::preview::enqueue_previews(&pool, &["visible".into()], false)
            .await
            .unwrap();
        let (first, other) = tokio::join!(claim_job(&pool, true), claim_job(&pool, true));
        let first = first.unwrap().unwrap();
        let other = other.unwrap().unwrap();
        assert_ne!(first.id, other.id);
        assert!([first.asset_id.as_str(), other.asset_id.as_str()].contains(&"visible"));
        assert_ne!(first.asset_id, "heavy");
        assert_ne!(other.asset_id, "heavy");
        assert_eq!(
            claim_job(&pool, false).await.unwrap().unwrap().asset_id,
            "heavy"
        );
        assert!(claim_job(&pool, true).await.unwrap().is_some());
        assert!(claim_job(&pool, true).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn native_failure_is_claimed_by_python_lane() {
        let pool = pool().await;
        asset(&pool, "broken", "jpg").await;
        job(&pool, "broken-job", "broken", "queued", 0).await;
        let job = claim_job(&pool, true).await.unwrap().unwrap();
        defer_to_python(&pool, &job).await.unwrap();
        assert!(claim_job(&pool, true).await.unwrap().is_none());
        let fallback = claim_job(&pool, false).await.unwrap().unwrap();
        assert_eq!(fallback.id, job.id);
        assert!(
            complete_job(&pool, &fallback, true, Some("local.jpg"), None)
                .await
                .unwrap()
        );
        assert_eq!(counts(&pool).await, (1, 1, 0, 0));
    }

    #[tokio::test]
    async fn native_failure_uses_persistent_python_image_protocol() {
        let pool = pool().await;
        asset(&pool, "fallback", "png").await;
        job(&pool, "fallback-job", "fallback", "queued", 0).await;
        let job = claim_job(&pool, true).await.unwrap().unwrap();
        let dir = std::env::temp_dir().join(format!("lumiadam-python-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("broken.png");
        std::fs::write(&source, b"invalid native image").unwrap();
        assert!(generate_image_thumb_async(
            source.clone(),
            dir.join("native.png"),
            Arc::new(tokio::sync::Semaphore::new(2))
        )
        .await
        .is_err());
        defer_to_python(&pool, &job).await.unwrap();
        let job = claim_job(&pool, false).await.unwrap().unwrap();
        let script = dir.join("worker.py");
        std::fs::write(
            &script,
            r#"import json,sys,pathlib
for line in sys.stdin:
    req=json.loads(line)
    if req['method']=='image_thumb':
        output=pathlib.Path(req['params']['output'])
        output.parent.mkdir(parents=True,exist_ok=True)
        output.write_bytes(b'python fallback')
        result={'ok':True,'output':str(output)}
    else:
        result={'ok':False,'error':'Wrong raster fallback method'}
    print(json.dumps({'id':req['id'],'result':result}),flush=True)
"#,
        )
        .unwrap();
        let python = std::env::var_os("OPENDAM_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("python"));
        let mut worker = None;
        let reply = run_python(
            &mut worker,
            &python,
            &script,
            &job,
            &source,
            &dir.join("output"),
        )
        .await;
        if reply.is_err() {
            if let Some(worker) = worker.as_mut() {
                worker.kill().await;
            }
        }
        let (thumb, model) = reply.unwrap();
        let pid = worker.as_ref().unwrap().child.id();
        let (again, _) = run_python(
            &mut worker,
            &python,
            &script,
            &job,
            &source,
            &dir.join("second"),
        )
        .await
        .unwrap();
        assert_eq!(worker.as_ref().unwrap().child.id(), pid);
        assert!(Path::new(again.as_ref().unwrap()).is_file());
        assert!(
            complete_job(&pool, &job, true, thumb.as_deref(), model.as_deref())
                .await
                .unwrap()
        );
        worker.as_mut().unwrap().kill().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn stale_completion_does_not_overwrite_rescan_or_partial_commit() {
        let pool = pool().await;
        asset(&pool, "changed", "jpg").await;
        job(&pool, "old-job", "changed", "queued", 0).await;
        let old = claim_job(&pool, true).await.unwrap().unwrap();
        sqlx::query("DELETE FROM preview_jobs WHERE id='old-job'; UPDATE assets SET modified_at=2,preview_status='pending',thumbnail_path=NULL WHERE id='changed'")
            .execute(&pool).await.unwrap();
        job(&pool, "new-job", "changed", "pending", 0).await;
        assert!(!complete_job(&pool, &old, true, Some("stale.jpg"), None)
            .await
            .unwrap());
        let current = claim_job(&pool, true).await.unwrap().unwrap();
        sqlx::query("CREATE TRIGGER fail_asset_update BEFORE UPDATE OF preview_status ON assets BEGIN SELECT RAISE(ABORT,'test failure'); END")
            .execute(&pool).await.unwrap();
        assert!(complete_job(&pool, &current, true, Some("fresh.jpg"), None)
            .await
            .is_err());
        assert_eq!(counts(&pool).await, (1, 0, 1, 0));
        let status: String =
            sqlx::query_scalar("SELECT status FROM preview_jobs WHERE id='new-job'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "processing");
        let thumb: Option<String> =
            sqlx::query_scalar("SELECT thumbnail_path FROM assets WHERE id='changed'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(thumb.is_none());
    }

    #[tokio::test]
    async fn native_images_wait_for_one_of_two_blocking_slots() {
        let dir = std::env::temp_dir().join(format!("lumiadam-slots-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.png");
        let output = dir.join("thumb.jpg");
        image::RgbImage::new(4, 4).save(&source).unwrap();
        let slots = Arc::new(tokio::sync::Semaphore::new(NATIVE_CONCURRENCY));
        let first = slots.clone().acquire_owned().await.unwrap();
        let second = slots.clone().acquire_owned().await.unwrap();
        let mut task = tokio::spawn(generate_image_thumb_async(source, output.clone(), slots));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(40), &mut task)
                .await
                .is_err()
        );
        assert!(!output.exists());
        drop(first);
        task.await.unwrap().unwrap();
        assert!(output.exists());
        drop(second);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn visible_requests_repair_legacy_or_missing_image_cache_without_replacing_valid_jobs() {
        let pool = pool().await;
        for id in ["legacy", "missing", "valid"] {
            asset(&pool, id, "jpg").await;
            job(&pool, &format!("{id}-job"), id, "done", 0).await;
        }
        let valid = std::env::temp_dir().join(format!("lumiadam-preview-{}.jpg", uuid::Uuid::new_v4()));
        std::fs::write(&valid, b"cached").unwrap();
        sqlx::query("UPDATE assets SET preview_status='done',thumbnail_path=CASE id WHEN 'legacy' THEN '.opendam/thumbs/9_thumb.jpg' WHEN 'valid' THEN ? ELSE NULL END")
            .bind(valid.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        crate::commands::preview::enqueue_previews(&pool, &["legacy".into(), "missing".into(), "valid".into()], false).await.unwrap();
        assert_eq!(counts(&pool).await, (3,1,2,0));
        let valid_id: String = sqlx::query_scalar("SELECT id FROM preview_jobs WHERE asset_id='valid'").fetch_one(&pool).await.unwrap();
        assert_eq!(valid_id, "valid-job");
        std::fs::remove_file(valid).unwrap();
    }

    #[tokio::test]
    async fn changed_version_without_job_deletion_is_requeued() {
        let pool = pool().await;
        asset(&pool,"renamed","jpg").await;
        job(&pool,"rename-job","renamed","queued",0).await;
        let old = claim_job(&pool,true).await.unwrap().unwrap();
        sqlx::query("UPDATE assets SET modified_at=modified_at+1,file_path='renamed.jpg' WHERE id='renamed'").execute(&pool).await.unwrap();
        assert!(!complete_job(&pool,&old,true,Some("stale.jpg"),None).await.unwrap());
        let next = claim_job(&pool,true).await.unwrap().expect("changed source should be queued again");
        assert_eq!(next.file_path,"renamed.jpg");
        assert_ne!(next.modified_at,old.modified_at);
    }

    #[test]
    fn previously_displayable_formats_have_local_preview_routes() {
        for ext in ["svg","avif","mp4","mov","avi","mkv","webm","m4v"] {
            assert!(is_previewable(ext),"{ext} needs a cache route");
        }
    }

}
