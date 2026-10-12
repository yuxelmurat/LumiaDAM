use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::sync::RwLock;

/// Sondaki ayırıcıları yok sayar; sürücü kökünde harfi, ağ paylaşımında paylaşım adını kullanır.
pub fn library_name(name: &str, root: &str) -> String {
    if !name.trim().is_empty() && !name.eq_ignore_ascii_case("New Library") {
        return name.to_string();
    }
    root.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next()
        .filter(|part| !part.is_empty()).unwrap_or(root).to_string()
}

#[derive(Clone)]
pub struct LibraryHandle {
    pub id: String,
    pub name: String,
    pub root_path: PathBuf,
    pub catalog_dir: PathBuf,
    pub scan_lock: Arc<tokio::sync::Mutex<()>>,
    pub scan_processed: Arc<AtomicU64>,
    pub scan_full: Arc<AtomicBool>,
    pub pool: Pool<Sqlite>,
}

pub struct Database {
    pub master_pool: Pool<Sqlite>,
    pub libraries: Arc<RwLock<HashMap<String, LibraryHandle>>>,
    pub pool: Pool<Sqlite>,
    pub storage_root: PathBuf,
    pub default_storage_root: PathBuf,
    pub storage_errors: Vec<String>,
    pub preview_wakeup: Arc<tokio::sync::Notify>,
}

/// Yerel diskler WAL; bilinmeyen konumlar DELETE kullanir.
fn library_journal_mode(db_path: &Path) -> SqliteJournalMode {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" { fn GetDriveTypeW(root_path: *const u16) -> u32; }
        let absolute = if db_path.is_absolute() { db_path.to_path_buf() } else {
            match std::env::current_dir() { Ok(dir) => dir.join(db_path), Err(_) => return SqliteJournalMode::Delete }
        };
        let value = absolute.to_string_lossy().replace('/', "\\");
        if value.starts_with("\\\\") { return SqliteJournalMode::Delete; }
        // Yerel sürücü içindeki junction bir ağ konumuna gidebilir; gerçek hedefi denetle.
        let Some(parent) = absolute.parent() else { return SqliteJournalMode::Delete; };
        let Ok(resolved) = std::fs::canonicalize(parent) else { return SqliteJournalMode::Delete; };
        let resolved = resolved.to_string_lossy().replace('/', "\\");
        let value = resolved.strip_prefix("\\\\?\\").unwrap_or(&resolved);
        if value.as_bytes().get(1) == Some(&b':') && value.as_bytes().get(2) == Some(&b'\\') {
            let drive: Vec<u16> = std::ffi::OsStr::new(&value[..3]).encode_wide().chain(Some(0)).collect();
            if matches!(unsafe { GetDriveTypeW(drive.as_ptr()) }, 2 | 3 | 6) { return SqliteJournalMode::Wal; }
        }
    }
    #[cfg(not(windows))]
    let _ = db_path;
    SqliteJournalMode::Delete
}

pub async fn connect_library_sqlite(db_path: &Path) -> Result<Pool<Sqlite>> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(library_journal_mode(db_path))
        .pragma("temp_store", "MEMORY")
        .pragma("cache_size", "-4096")
        .foreign_keys(false)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(30)); // 30s for slow NAS writes

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect_with(options)
        .await?;

    // Execute the per-library schema
    sqlx::query(include_str!("../migrations/002_library_schema.sql"))
        .execute(&pool)
        .await?;

    // Backward compatibility: ensure dir_mtimes table exists
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS dir_mtimes (dir_path TEXT PRIMARY KEY, mtime INTEGER NOT NULL)"
    )
    .execute(&pool)
    .await;

    // Backward compatibility migration for subcollections
    let _ = sqlx::query(
        "ALTER TABLE collections ADD COLUMN parent_id TEXT REFERENCES collections(id) ON DELETE CASCADE"
    )
    .execute(&pool)
    .await;

    // Backward compatibility migration for is_favorite
    let _ = sqlx::query(
        "ALTER TABLE assets ADD COLUMN is_favorite INTEGER NOT NULL DEFAULT 0"
    )
    .execute(&pool)
    .await;
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_assets_is_favorite ON assets(is_favorite)"
    )
    .execute(&pool)
    .await;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_assets_path_nocase ON assets(file_path COLLATE NOCASE)")
        .execute(&pool).await?;

    crate::folders::initialize(&pool).await?;
    crate::preview::initialize(&pool).await?;
    crate::search_index::initialize(&pool).await?;
    Ok(pool)
}

impl Database {
    pub async fn new(master_db_url: &str, default_storage_root: PathBuf) -> Result<Self> {
        let master_path = Path::new(master_db_url);
        let master_options = SqliteConnectOptions::new()
            .filename(master_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Delete) // consistent with library DBs
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(30));

        let master_pool = SqlitePoolOptions::new()
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect_with(master_options)
            .await?;

        // Ensure libraries table exists in master DB
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS libraries (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                root_paths  TEXT NOT NULL,
                created_at  INTEGER NOT NULL,
                asset_count INTEGER NOT NULL DEFAULT 0
            );
            "#
        )
        .execute(&master_pool)
        .await?;

        let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
            sqlx::query_as("PRAGMA table_info(libraries)").fetch_all(&master_pool).await?;
        if !columns.iter().any(|c| c.1 == "catalog_path") {
            sqlx::query("ALTER TABLE libraries ADD COLUMN catalog_path TEXT")
                .execute(&master_pool).await?;
        }
        sqlx::query("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&master_pool).await?;
        let selected_root: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'storage_root'")
            .fetch_optional(&master_pool).await?;
        let storage_root = selected_root.map(PathBuf::from).unwrap_or_else(|| default_storage_root.clone());

        // Also ensure library schema tables exist on master_pool as fallback
        let _ = sqlx::query(include_str!("../migrations/002_library_schema.sql"))
            .execute(&master_pool)
            .await;

        let _ = sqlx::query("ALTER TABLE assets ADD COLUMN is_favorite INTEGER NOT NULL DEFAULT 0")
            .execute(&master_pool)
            .await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_assets_is_favorite ON assets(is_favorite)")
            .execute(&master_pool)
            .await;

        let mut db = Self {
            pool: master_pool.clone(), master_pool,
            libraries: Arc::new(RwLock::new(HashMap::new())),
            storage_root, default_storage_root, storage_errors: Vec::new(),
            preview_wakeup: Arc::new(tokio::sync::Notify::new()),
        };
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, name, root_paths FROM libraries"
        ).fetch_all(&db.master_pool).await?;
        for (id, name, paths) in rows {
            let paths: Vec<String> = serde_json::from_str(&paths)?;
            if let Some(root) = paths.first() {
                let derived_name = library_name(&name, root);
                if derived_name != name {
                    sqlx::query("UPDATE libraries SET name = ? WHERE id = ?")
                        .bind(&derived_name).bind(&id).execute(&db.master_pool).await?;
                }
                let name = derived_name;
                if let Err(error) = db.register_library(&id, &name, Path::new(root)).await {
                    let message = format!("{}: {:#}", name, error);
                    tracing::error!("Cannot open catalog: {}", message);
                    db.storage_errors.push(message);
                }
            }
        }

        Ok(db)
    }

    pub async fn sync_library_metadata(&self, lib_pool: &Pool<Sqlite>) {
        // 1. Sync tags from master_pool to lib_pool
        if let Ok(tags) = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
            "SELECT id, name, color, parent_id FROM tags"
        ).fetch_all(&self.master_pool).await {
            for (id, name, color, parent_id) in tags {
                let _ = sqlx::query("INSERT OR IGNORE INTO tags (id, name, color, parent_id) VALUES (?, ?, ?, ?)")
                    .bind(&id).bind(&name).bind(&color).bind(&parent_id)
                    .execute(lib_pool).await;
            }
        }

        // 2. Sync tags from lib_pool to master_pool
        if let Ok(tags) = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
            "SELECT id, name, color, parent_id FROM tags"
        ).fetch_all(lib_pool).await {
            for (id, name, color, parent_id) in tags {
                let _ = sqlx::query("INSERT OR IGNORE INTO tags (id, name, color, parent_id) VALUES (?, ?, ?, ?)")
                    .bind(&id).bind(&name).bind(&color).bind(&parent_id)
                    .execute(&self.master_pool).await;
            }
        }

        // 3. Sync collections from master_pool to lib_pool
        if let Ok(cols) = sqlx::query_as::<_, (String, String, String, Option<String>, bool, Option<String>, i64)>(
            "SELECT id, name, description, parent_id, is_smart, filter_spec, created_at FROM collections"
        ).fetch_all(&self.master_pool).await {
            for (id, name, desc, parent_id, is_smart, filter_spec, created_at) in cols {
                let _ = sqlx::query("INSERT OR IGNORE INTO collections (id, name, description, parent_id, is_smart, filter_spec, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
                    .bind(&id).bind(&name).bind(&desc).bind(&parent_id).bind(is_smart).bind(&filter_spec).bind(created_at)
                    .execute(lib_pool).await;
            }
        }

        // 4. Sync collections from lib_pool to master_pool
        if let Ok(cols) = sqlx::query_as::<_, (String, String, String, Option<String>, bool, Option<String>, i64)>(
            "SELECT id, name, description, parent_id, is_smart, filter_spec, created_at FROM collections"
        ).fetch_all(lib_pool).await {
            for (id, name, desc, parent_id, is_smart, filter_spec, created_at) in cols {
                let _ = sqlx::query("INSERT OR IGNORE INTO collections (id, name, description, parent_id, is_smart, filter_spec, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
                    .bind(&id).bind(&name).bind(&desc).bind(&parent_id).bind(is_smart).bind(&filter_spec).bind(created_at)
                    .execute(&self.master_pool).await;
            }
        }
    }

    pub async fn register_library(&self, id: &str, name: &str, root_path: &Path) -> Result<LibraryHandle> {
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT catalog_path FROM libraries WHERE id = ?"
        ).bind(id).fetch_optional(&self.master_pool).await?.flatten();
        let catalog_dir = if let Some(previous) = previous {
            let previous = PathBuf::from(previous);
            anyhow::ensure!(previous.join("library.sqlite").try_exists()?, "Saved catalog was not found; no empty catalog was created");
            if previous.parent() == Some(self.storage_root.as_path()) {
                previous
            } else {
                crate::storage::prepare_catalog(&self.storage_root, name, id, Some(&previous)).await?
            }
        } else {
            // İlk aktarım için kaynak erişilebilir olmalı; sonraki açılışlar NAS'a bağlı değildir.
            anyhow::ensure!(root_path.is_dir(), "Source folder is unavailable; connect it for the first transfer");
            let legacy = root_path.join(".opendam");
            let source = if legacy.join("library.sqlite").try_exists()? { Some(legacy.as_path()) } else { None };
            crate::storage::prepare_catalog(&self.storage_root, name, id, source).await?
        };
        let lib_db_file = catalog_dir.join("library.sqlite");
        let lib_pool = connect_library_sqlite(&lib_db_file).await?;
        sqlx::query("UPDATE libraries SET catalog_path = ? WHERE id = ?")
            .bind(catalog_dir.to_string_lossy().as_ref()).bind(id).execute(&self.master_pool).await?;

        // Sync metadata immediately
        self.sync_library_metadata(&lib_pool).await;

        let handle = LibraryHandle {
            id: id.to_string(),
            name: name.to_string(),
            root_path: root_path.to_path_buf(),
            catalog_dir,
            scan_lock: Arc::new(tokio::sync::Mutex::new(())),
            scan_processed: Arc::new(AtomicU64::new(0)),
            scan_full: Arc::new(AtomicBool::new(false)),
            pool: lib_pool,
        };

        let mut libs = self.libraries.write().await;
        libs.insert(id.to_string(), handle.clone());

        Ok(handle)
    }

    pub async fn unregister_library(&self, id: &str) -> Result<()> {
        let mut libs = self.libraries.write().await;
        libs.remove(id);
        Ok(())
    }

    pub async fn get_library_handle(&self, id: Option<&str>) -> Option<LibraryHandle> {
        let libs = self.libraries.read().await;
        if let Some(target_id) = id {
            return libs.get(target_id).cloned();
        }
        libs.values().next().cloned()
    }

    pub async fn get_library_root(&self, id: Option<&str>) -> Option<PathBuf> {
        self.get_library_handle(id).await.map(|h| h.root_path)
    }

    pub async fn get_pool(&self, id: Option<&str>) -> Pool<Sqlite> {
        if let Some(h) = self.get_library_handle(id).await {
            h.pool
        } else {
            self.pool.clone()
        }
    }

    pub async fn get_active_pool(&self) -> Pool<Sqlite> {
        self.get_pool(None).await
    }

    pub async fn get_all_libraries(&self) -> Vec<LibraryHandle> {
        let libs = self.libraries.read().await;
        libs.values().cloned().collect()
    }
}

/// Helper to resolve a stored relative path to an absolute path for the frontend
pub fn resolve_path(root: &Path, rel_or_abs: &str) -> String {
    let p = Path::new(rel_or_abs);
    if p.is_absolute() {
        rel_or_abs.to_string()
    } else {
        root.join(p).to_string_lossy().to_string()
    }
}

/// Eski ortak önbellekte aynı adlı dosyalar tek `thumbs/ad_thumb.jpg` yolunu paylaşır.
pub fn is_legacy_thumbnail(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.rsplit_once('/')
        .and_then(|(parent, _)| parent.rsplit('/').next()) == Some("thumbs")
}

/// Helper to convert an absolute path to a portable relative path for storage
pub fn make_relative_path(root: &Path, p: &Path) -> String {
    if let Ok(rel) = p.strip_prefix(root) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        p.to_string_lossy().replace('\\', "/")
    }
}

#[cfg(test)]
mod journal_tests {
    use super::*;
    #[test]
    fn unc_and_extended_unknown_locations_never_enable_wal() {
        for path in [r"\\nas\share\catalog.sqlite", r"\\?\UNC\nas\share\catalog.sqlite", r"\\?\Volume{unknown}\catalog.sqlite"] {
            assert!(matches!(library_journal_mode(Path::new(path)), SqliteJournalMode::Delete));
        }
    }
}
