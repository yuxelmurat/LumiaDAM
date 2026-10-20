use crate::{db::resolve_path, AppState};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    hash::Hasher,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::Emitter;

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
struct ScanGuard;
impl Drop for ScanGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFile {
    pub id: String,
    pub library_id: String,
    pub file_name: String,
    pub file_path: String,
    pub size_bytes: u64,
    pub thumbnail_path: Option<String>,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    root: PathBuf,
    #[serde(skip)]
    modified: i64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub kind: String,
    pub files: Vec<DuplicateFile>,
    pub distance: Option<u32>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateResult {
    pub groups: Vec<DuplicateGroup>,
    pub group_count: usize,
    pub processed: usize,
    pub total: usize,
    pub skipped: usize,
    pub cancelled: bool,
    pub limited: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub processed: usize,
    pub total: usize,
    pub skipped: usize,
    pub phase: String,
}

fn fingerprint(
    path: &Path,
    size: u64,
    modified: i64,
    mut notify: impl FnMut(u64),
) -> anyhow::Result<u64> {
    let mut file = std::fs::File::open(path)?;
    let before = file.metadata()?;
    let timestamp = |meta: &std::fs::Metadata| -> anyhow::Result<i64> {
        Ok(meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as i64)
    };
    anyhow::ensure!(
        before.is_file() && before.len() == size && timestamp(&before)? == modified,
        "Source changed; rescan before comparing"
    );
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = vec![0u8; 128 * 1024];
    let mut read_total = 0;
    let mut last = std::time::Instant::now();
    loop {
        anyhow::ensure!(!CANCEL.load(Ordering::SeqCst), "Scan cancelled");
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.write(&buffer[..read]);
        read_total += read as u64;
        if last.elapsed().as_millis() >= 250 {
            notify(read_total);
            last = std::time::Instant::now();
        }
    }
    anyhow::ensure!(
        read_total == size && timestamp(&file.metadata()?)? == timestamp(&before)?,
        "File changed while being compared"
    );
    Ok(hash.finish())
}

/// Hash yalnız adayları daraltır; çakışmalar byte karşılaştırması ile elenir.
fn identical(left: &Path, right: &Path, mut notify: impl FnMut(u64)) -> anyhow::Result<bool> {
    let mut a = std::io::BufReader::new(std::fs::File::open(left)?);
    let mut b = std::io::BufReader::new(std::fs::File::open(right)?);
    let mut x = vec![0; 128 * 1024];
    let mut y = vec![0; 128 * 1024];
    let mut read_total = 0;
    let mut last = std::time::Instant::now();
    loop {
        anyhow::ensure!(!CANCEL.load(Ordering::SeqCst), "Scan cancelled");
        let n = a.read(&mut x)?;
        if n == 0 {
            return Ok(b.read(&mut y)? == 0);
        }
        let mut got = 0;
        while got < n {
            let read = b.read(&mut y[got..n])?;
            if read == 0 {
                return Ok(false);
            }
            got += read;
        }
        if x[..n] != y[..n] {
            return Ok(false);
        }
        read_total += n as u64;
        if last.elapsed().as_millis() >= 250 {
            notify(read_total);
            last = std::time::Instant::now();
        }
    }
}

pub(crate) fn image_hash(path: &Path) -> anyhow::Result<(u64, f64)> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?;
    anyhow::ensure!(image.width() > 0 && image.height() > 0, "Empty image");
    let ratio = image.width() as f64 / image.height() as f64;
    let tiny = image
        .resize_exact(9, 8, image::imageops::FilterType::Triangle)
        .to_luma8();
    let range = tiny.as_raw().iter().max().unwrap() - tiny.as_raw().iter().min().unwrap();
    anyhow::ensure!(
        range > 8,
        "Nearly uniform previews are excluded from similarity matching"
    );
    let mut hash = 0;
    for y in 0..8 {
        for x in 0..8 {
            if tiny.get_pixel(x, y).0[0] > tiny.get_pixel(x + 1, y).0[0] {
                hash |= 1u64 << (y * 8 + x);
            }
        }
    }
    Ok((hash, ratio))
}

pub async fn scan(
    state: &AppState,
    mode: String,
    library_ids: Option<Vec<String>>,
) -> anyhow::Result<DuplicateResult> {
    anyhow::ensure!(
        matches!(mode.as_str(), "exact" | "similar"),
        "Unknown comparison mode"
    );
    anyhow::ensure!(
        RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok(),
        "A duplicate scan is already running"
    );
    let guard = ScanGuard;
    CANCEL.store(false, Ordering::SeqCst);
    let mut files = Vec::new();
    for library in state.db.get_all_libraries().await {
        if library_ids
            .as_ref()
            .is_some_and(|ids| !ids.is_empty() && !ids.contains(&library.id))
        {
            continue;
        }
        let mut roots = crate::watcher::source_roots(&state.db, &library).await?;
        roots.push(library.catalog_dir.join("exports"));
        let rows: Vec<(String,String,String,i64,i64,Option<String>,Option<String>,Option<i64>,Option<i64>)>=sqlx::query_as("SELECT a.id,a.file_path,a.file_name,a.size_bytes,a.modified_at,a.thumbnail_path,o.file_name,o.size_bytes,o.modified_at FROM assets a LEFT JOIN offline_assets o ON o.asset_id=a.id ORDER BY a.id")
            .fetch_all(&library.pool).await?;
        for (id, path, name, size, modified, thumb, cached, cached_size, cached_modified) in rows {
            let original = PathBuf::from(resolve_path(&library.root_path, &path));
            let mut working = original.clone();
            let mut root = roots
                .iter()
                .find(|root| crate::storage::path_is_within(&original, root))
                .cloned()
                .unwrap_or_else(|| library.root_path.clone());
            let mut expected_modified = modified;
            if let Some(cached) = cached {
                if cached_size == Some(size) && cached_modified == Some(modified) {
                    let candidate = crate::workflow::offline_path(&library, &id, &cached)?;
                    if let Ok(meta) = tokio::fs::metadata(&candidate).await {
                        if meta.is_file() && meta.len() == size as u64 {
                            expected_modified = meta
                                .modified()?
                                .duration_since(std::time::UNIX_EPOCH)?
                                .as_millis() as i64;
                            working = candidate;
                            root = library.catalog_dir.join("offline");
                        }
                    }
                }
            }
            files.push(DuplicateFile {
                id,
                library_id: library.id.clone(),
                file_name: name,
                file_path: original.to_string_lossy().into_owned(),
                size_bytes: size.max(0) as u64,
                thumbnail_path: thumb
                    .filter(|path| !crate::db::is_legacy_thumbnail(path))
                    .map(|path| resolve_path(&library.root_path, &path)),
                path: working,
                root,
                modified: expected_modified,
            });
        }
    }
    if mode == "exact" {
        let mut sizes = HashMap::new();
        for file in &files {
            *sizes.entry(file.size_bytes).or_insert(0usize) += 1;
        }
        files.retain(|file| sizes[&file.size_bytes] > 1);
    } else {
        files.retain(|file| {
            file.thumbnail_path
                .as_ref()
                .is_some_and(|p| !p.ends_with(".glb"))
        });
    }
    let app = state.app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        compare_files(files, &mode, |progress| {
            let _ = app.emit("duplicates:progress", progress);
        })
    })
    .await?
}

fn compare_files(
    files: Vec<DuplicateFile>,
    mode: &str,
    notify: impl Fn(ScanProgress),
) -> anyhow::Result<DuplicateResult> {
    let mut result = DuplicateResult {
        total: files.len(),
        ..Default::default()
    };
    let progress = |result: &DuplicateResult, phase: &str| {
        notify(ScanProgress {
            processed: result.processed,
            total: result.total,
            skipped: result.skipped,
            phase: phase.into(),
        })
    };
    progress(&result, "Preparing comparison");
    let mut last = std::time::Instant::now();
    let mut buckets: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    let mut hashes: Vec<Option<(u64, f64)>> = vec![None; files.len()];
    for (index, file) in files.iter().enumerate() {
        if CANCEL.load(Ordering::SeqCst) {
            result.cancelled = true;
            break;
        }
        let found = if mode == "exact" {
            (|| -> anyhow::Result<()> {
                if result.processed == 0 || last.elapsed().as_millis() >= 250 {
                    progress(&result, &format!("Reading {}", file.file_name));
                    last = std::time::Instant::now();
                }
                let path = file.path.canonicalize()?;
                anyhow::ensure!(
                    path.starts_with(file.root.canonicalize()?),
                    "File resolves outside registered storage"
                );
                let hash = fingerprint(&path, file.size_bytes, file.modified, |bytes| {
                    progress(
                        &result,
                        &format!(
                            "Reading {} ({}%)",
                            file.file_name,
                            bytes.saturating_mul(100) / file.size_bytes.max(1)
                        ),
                    )
                })?;
                buckets
                    .entry((file.size_bytes, hash))
                    .or_default()
                    .push(index);
                Ok(())
            })()
        } else {
            image_hash(Path::new(file.thumbnail_path.as_ref().unwrap()))
                .map(|hash| hashes[index] = Some(hash))
        };
        result.processed += 1;
        if found.is_err() {
            result.skipped += 1;
        }
        if last.elapsed().as_millis() >= 200 {
            progress(
                &result,
                if mode == "exact" {
                    "Reading file contents"
                } else {
                    "Comparing cached previews"
                },
            );
            last = std::time::Instant::now();
        }
    }
    if mode == "exact" {
        progress(&result, "Verifying identical file contents");
        let verification_total = buckets
            .values()
            .filter(|indices| indices.len() > 1)
            .map(Vec::len)
            .sum();
        let mut verified_files = 0;
        for indices in buckets.values().filter(|indices| indices.len() > 1) {
            let mut verified: Vec<Vec<usize>> = Vec::new();
            for index in indices {
                if CANCEL.load(Ordering::SeqCst) {
                    result.cancelled = true;
                    break;
                }
                let mut matched = false;
                for group in &mut verified {
                    match identical(&files[group[0]].path, &files[*index].path, |bytes| {
                        notify(ScanProgress {
                            processed: verified_files,
                            total: verification_total,
                            skipped: result.skipped,
                            phase: format!(
                                "Verifying {} ({}%)",
                                files[*index].file_name,
                                bytes.saturating_mul(100) / files[*index].size_bytes.max(1)
                            ),
                        })
                    }) {
                        Ok(true) => {
                            group.push(*index);
                            matched = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(_) => {
                            result.skipped += 1;
                            matched = true;
                            break;
                        }
                    }
                }
                if !matched {
                    verified.push(vec![*index]);
                }
                verified_files += 1;
                if last.elapsed().as_millis() >= 250 {
                    notify(ScanProgress {
                        processed: verified_files,
                        total: verification_total,
                        skipped: result.skipped,
                        phase: "Verifying identical contents".into(),
                    });
                    last = std::time::Instant::now();
                }
            }
            for group in verified.into_iter().filter(|group| group.len() > 1) {
                result.groups.push(DuplicateGroup {
                    kind: "exact".into(),
                    files: group.into_iter().map(|i| files[i].clone()).collect(),
                    distance: None,
                });
            }
            if result.cancelled {
                break;
            }
        }
    } else {
        // Sekiz bant, altı bitlik farkta en az bir eşleşen bant bulunmasını sağlar.
        let mut bands: HashMap<(u8, u8), Vec<usize>> = HashMap::new();
        let mut grouped: HashMap<usize, usize> = HashMap::new();
        let mut representatives: Vec<usize> = Vec::new();
        for index in 0..files.len() {
            if CANCEL.load(Ordering::SeqCst) {
                result.cancelled = true;
                break;
            }
            let Some((hash, ratio)) = hashes[index] else {
                continue;
            };
            let mut candidates = HashSet::new();
            for band in 0..8 {
                if let Some(indices) = bands.get(&(band, ((hash >> (band * 8)) & 255) as u8)) {
                    candidates.extend(indices.iter().copied());
                }
            }
            let mut matches = Vec::new();
            let mut distance = 0;
            let mut existing = None;
            for candidate in candidates {
                let (other, aspect) = hashes[candidate].unwrap();
                let delta = (hash ^ other).count_ones();
                if delta <= 6 && (ratio / aspect - 1.0).abs() <= 0.1 {
                    if let Some(group) = grouped.get(&candidate) {
                        let (reference, aspect) = hashes[representatives[*group]].unwrap();
                        if (hash ^ reference).count_ones() <= 6
                            && (ratio / aspect - 1.0).abs() <= 0.1
                        {
                            existing = Some(*group);
                            break;
                        }
                    } else {
                        matches.push(candidate);
                        distance = distance.max(delta);
                    }
                }
            }
            if let Some(group) = existing {
                result.groups[group].files.push(files[index].clone());
                grouped.insert(index, group);
            } else if !matches.is_empty() {
                matches.push(index);
                for member in &matches {
                    grouped.insert(*member, result.groups.len());
                }
                representatives.push(index);
                result.groups.push(DuplicateGroup {
                    kind: "similar".into(),
                    files: matches.into_iter().map(|i| files[i].clone()).collect(),
                    distance: Some(distance),
                });
            }
            for band in 0..8 {
                bands
                    .entry((band, ((hash >> (band * 8)) & 255) as u8))
                    .or_default()
                    .push(index);
            }
        }
    }
    result.cancelled |= CANCEL.load(Ordering::SeqCst);
    result.groups.sort_by_key(|group| {
        std::cmp::Reverse(group.files.iter().map(|file| file.size_bytes).sum::<u64>())
    });
    result.group_count = result.groups.len();
    result.limited = result.group_count > 500;
    result.groups.truncate(500);
    progress(
        &result,
        if result.cancelled {
            "Cancelled"
        } else {
            "Complete"
        },
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_size_or_hash_candidate_does_not_prove_equal_contents() {
        let root =
            std::env::temp_dir().join(format!("lumiadam-duplicates-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let a = root.join("a.zip");
        let b = root.join("b.zip");
        let c = root.join("c.zip");
        std::fs::write(&a, vec![7; 300_000]).unwrap();
        std::fs::write(&b, vec![8; 300_000]).unwrap();
        std::fs::write(&c, vec![7; 300_000]).unwrap();
        CANCEL.store(false, Ordering::SeqCst);
        assert!(!identical(&a, &b, |_| {}).unwrap());
        assert!(identical(&a, &c, |_| {}).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn candidate(root: &Path, name: &str) -> DuplicateFile {
        let path = root.join(name);
        let metadata = std::fs::metadata(&path).unwrap();
        DuplicateFile {
            id: name.into(),
            library_id: "library".into(),
            file_name: name.into(),
            file_path: path.to_string_lossy().into_owned(),
            size_bytes: metadata.len(),
            thumbnail_path: Some(path.to_string_lossy().into_owned()),
            path,
            root: root.into(),
            modified: metadata
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
        }
    }
    #[test]
    fn exact_scan_checks_contents_skips_missing_sources_and_preserves_files() {
        let root = std::env::temp_dir().join(format!("lumiadam-compare-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for (name, value) in [
            ("9.zip", b"first archive"),
            ("copy.zip", b"first archive"),
            ("other.zip", b"other archive"),
            ("missing.zip", b"first archive"),
        ] {
            std::fs::write(root.join(name), value).unwrap();
        }
        let files =
            ["9.zip", "copy.zip", "other.zip", "missing.zip"].map(|name| candidate(&root, name));
        std::fs::remove_file(root.join("missing.zip")).unwrap();
        let result = compare_files(files.into(), "exact", |_| {}).unwrap();
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].files.len(), 2);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.processed, 4);
        assert_eq!(std::fs::read(root.join("9.zip")).unwrap(), b"first archive");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn similar_render_group_includes_three_copies_and_excludes_flat_or_opposite_previews() {
        let root = std::env::temp_dir().join(format!("lumiadam-similar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let gradient = image::RgbImage::from_fn(72, 64, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 3) as u8, 30])
        });
        for name in ["one.png", "two.png", "three.png"] {
            gradient.save(root.join(name)).unwrap();
        }
        image::RgbImage::from_fn(72, 64, |x, y| {
            image::Rgb([((71 - x) * 3) as u8, (y * 3) as u8, 30])
        })
        .save(root.join("different.png"))
        .unwrap();
        image::RgbImage::from_pixel(72, 64, image::Rgb([90, 90, 90]))
            .save(root.join("flat.png"))
            .unwrap();
        let files = [
            "one.png",
            "two.png",
            "three.png",
            "different.png",
            "flat.png",
        ]
        .map(|name| candidate(&root, name));
        let result = compare_files(files.into(), "similar", |_| {}).unwrap();
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].files.len(), 3);
        assert_eq!(result.skipped, 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
