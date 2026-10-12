use super::*;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lumiadam-watch-{}", Uuid::new_v4()));
        std::fs::create_dir_all(path.join("source/kitchen")).unwrap();
        std::fs::create_dir_all(path.join("source/kitchen-old")).unwrap();
        Self(path)
    }
    async fn open(&self) -> (Database, String) {
        let db = Database::new(self.0.join("master.sqlite").to_str().unwrap(), self.0.join("catalogs")).await.unwrap();
        let id = Uuid::new_v4().to_string();
        let source = self.0.join("source");
        sqlx::query("INSERT INTO libraries(id,name,root_paths,created_at) VALUES(?,'Test',?,1)")
            .bind(&id).bind(serde_json::to_string(&vec![&source]).unwrap()).execute(&db.master_pool).await.unwrap();
        db.register_library(&id, "Test", &source).await.unwrap();
        (db, id)
    }
    fn path(&self, relative: &str) -> String { self.0.join(relative).to_string_lossy().into_owned() }
    fn write(&self, relative: &str, bytes: &[u8]) { std::fs::write(self.0.join(relative), bytes).unwrap(); }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}
async fn close(db: Database) {
    for library in db.get_all_libraries().await { library.pool.close().await; }
    db.master_pool.close().await;
}
async fn paths(db: &Database, id: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT file_path FROM assets ORDER BY file_path")
        .fetch_all(&db.get_library_handle(Some(id)).await.unwrap().pool).await.unwrap()
}

#[tokio::test]
async fn subtree_reconciles_create_rename_delete_without_touching_sibling() {
    let f = Fixture::new();
    f.write("source/kitchen/a.txt", b"a");
    f.write("source/kitchen/delete.txt", b"delete");
    f.write("source/kitchen-old/keep.txt", b"keep");
    let (db, id) = f.open().await;
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    std::fs::rename(f.path("source/kitchen/a.txt"), f.path("source/kitchen/renamed.txt")).unwrap();
    std::fs::remove_file(f.path("source/kitchen/delete.txt")).unwrap();
    f.write("source/kitchen/new.txt", b"new");
    index_paths(&db, &id, &[f.path("source/kitchen")]).await.unwrap();
    assert_eq!(paths(&db, &id).await, ["kitchen-old/keep.txt", "kitchen/new.txt", "kitchen/renamed.txt"]);
    let folders: Vec<String> = sqlx::query_scalar("SELECT path FROM folders ORDER BY path")
        .fetch_all(&db.get_library_handle(Some(&id)).await.unwrap().pool).await.unwrap();
    assert_eq!(folders, ["kitchen", "kitchen-old"]);
    assert!(Path::new(&f.path("source/kitchen-old/keep.txt")).exists());
    close(db).await;
}

#[tokio::test]
async fn changed_source_invalidates_preview_preserving_user_metadata() {
    let f = Fixture::new();
    f.write("source/kitchen/photo.jpg", b"first");
    let (db, id) = f.open().await;
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    let library = db.get_library_handle(Some(&id)).await.unwrap();
    let asset_id: String = sqlx::query_scalar("SELECT id FROM assets").fetch_one(&library.pool).await.unwrap();
    let thumb_dir = library.catalog_dir.join("thumbs").join(&asset_id);
    std::fs::create_dir_all(&thumb_dir).unwrap();
    let thumb = thumb_dir.join("photo_thumb.jpg");
    std::fs::write(&thumb, b"old").unwrap();
    sqlx::query("UPDATE assets SET rating=5,description='keep',thumbnail_path=?,preview_status='done',metadata='{\"preview_model\":\"stale.glb\",\"author\":\"keep\"}'")
        .bind(thumb.to_string_lossy().as_ref()).execute(&library.pool).await.unwrap();
    sqlx::query("INSERT INTO tags(id,name) VALUES('tag','keep'); INSERT INTO asset_tags(asset_id,tag_id) SELECT id,'tag' FROM assets; UPDATE preview_jobs SET status='processing';")
        .execute(&library.pool).await.unwrap();
    let old_job: String = sqlx::query_scalar("SELECT id FROM preview_jobs").fetch_one(&library.pool).await.unwrap();
    f.write("source/kitchen/photo.jpg", b"changed bytes");
    index_paths(&db, &id, &[f.path("source/kitchen")]).await.unwrap();
    let row: (String, i64, String, Option<String>, String, String) = sqlx::query_as("SELECT id,rating,description,thumbnail_path,preview_status,metadata FROM assets")
        .fetch_one(&library.pool).await.unwrap();
    assert_eq!(row.0, asset_id);
    assert_eq!(row.1, 5);
    assert_eq!(row.2, "keep");
    assert_eq!(row.3, None);
    assert_eq!(row.4, "pending");
    let metadata: serde_json::Value = serde_json::from_str(&row.5).unwrap();
    assert_eq!(metadata["author"], "keep");
    assert!(metadata.get("preview_model").is_none());
    let jobs: Vec<(String, String)> = sqlx::query_as("SELECT id,status FROM preview_jobs").fetch_all(&library.pool).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_ne!(jobs[0].0, old_job);
    assert_eq!(jobs[0].1, "pending");
    let tags: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM asset_tags").fetch_one(&library.pool).await.unwrap();
    assert_eq!(tags, 1);
    close(db).await;
}

#[tokio::test]
async fn unchanged_source_causes_no_asset_or_folder_writes() {
    let f = Fixture::new();
    f.write("source/kitchen/a.txt", b"unchanged");
    let (db, id) = f.open().await;
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    let library = db.get_library_handle(Some(&id)).await.unwrap();
    sqlx::query("CREATE TABLE writes(n INTEGER); CREATE TRIGGER test_assets_write AFTER UPDATE ON assets BEGIN INSERT INTO writes VALUES(1); END; CREATE TRIGGER test_folders_delete AFTER DELETE ON folders BEGIN INSERT INTO writes VALUES(1); END; CREATE TRIGGER test_folders_insert AFTER INSERT ON folders BEGIN INSERT INTO writes VALUES(1); END;")
        .execute(&library.pool).await.unwrap();
    index_paths(&db, &id, &[f.path("source/kitchen")]).await.unwrap();
    let writes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM writes").fetch_one(&library.pool).await.unwrap();
    assert_eq!(writes, 0);
    close(db).await;
}

#[tokio::test]
async fn unavailable_roots_and_failed_scopes_never_delete_catalog() {
    let f = Fixture::new();
    f.write("source/kitchen/a.txt", b"keep");
    let (db, id) = f.open().await;
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    assert!(index_paths(&db, &id, &[f.path("source/kitchen"), f.path("source/missing")]).await.is_err());
    assert_eq!(paths(&db, &id).await, ["kitchen/a.txt"]);
    std::fs::rename(f.path("source"), f.path("offline")).unwrap();
    assert!(index_paths(&db, &id, &[f.path("source")]).await.is_err());
    assert_eq!(paths(&db, &id).await, ["kitchen/a.txt"]);
    close(db).await;
}

#[tokio::test]
async fn rejects_paths_outside_registered_sources_and_catalogs() {
    let f = Fixture::new();
    let (db, id) = f.open().await;
    std::fs::create_dir_all(f.0.join("outside")).unwrap();
    f.write("outside/external.txt", b"not a source");
    assert!(index_paths(&db, &id, &[f.path("outside")]).await.is_err());
    let library = db.get_library_handle(Some(&id)).await.unwrap();
    assert!(index_paths(&db, &id, &[library.catalog_dir.to_string_lossy().into_owned()]).await.is_err());
    assert!(paths(&db, &id).await.is_empty());
    close(db).await;
}

#[tokio::test]
async fn failed_walk_preserves_missing_assets_and_folders() {
    let f = Fixture::new();
    f.write("source/kitchen/keep.txt", b"catalog entry");
    let (db, id) = f.open().await;
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    std::fs::remove_file(f.path("source/kitchen/keep.txt")).unwrap();
    let link = f.0.join("source/kitchen/loop");
    let target = f.0.join("source/kitchen");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"]).arg(link.to_string_lossy().replace('/', "\\")).arg(target.to_string_lossy().replace('/', "\\"))
            .output().unwrap();
        assert!(output.status.success(), "Failed to create the temporary traversal-loop junction: {} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }
    assert!(index_paths(&db, &id, &[f.path("source/kitchen")]).await.is_err());
    assert_eq!(paths(&db, &id).await, ["kitchen/keep.txt"]);
    let folders: Vec<String> = sqlx::query_scalar("SELECT path FROM folders ORDER BY path")
        .fetch_all(&db.get_library_handle(Some(&id)).await.unwrap().pool).await.unwrap();
    assert_eq!(folders, ["kitchen", "kitchen-old"]);
    close(db).await;
}

#[tokio::test]
async fn primary_root_scan_preserves_other_registered_root_catalog_entries() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.0.join("second")).unwrap();
    f.write("source/kitchen/local.txt", b"primary");
    f.write("second/remote.txt", b"second");
    let (db, id) = f.open().await;
    sqlx::query("UPDATE libraries SET root_paths=? WHERE id=?")
        .bind(serde_json::to_string(&vec![f.path("source"), f.path("second")]).unwrap())
        .bind(&id).execute(&db.master_pool).await.unwrap();
    index_paths(&db, &id, &[f.path("source"), f.path("second")]).await.unwrap();
    std::fs::remove_file(f.path("second/remote.txt")).unwrap();
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    assert_eq!(paths(&db, &id).await.len(), 2);
    index_paths(&db, &id, &[f.path("second")]).await.unwrap();
    assert_eq!(paths(&db, &id).await, ["kitchen/local.txt"]);
    close(db).await;
}

#[test]
fn native_events_are_scoped_and_pending_directories_stay_bounded() {
    let f = Fixture::new();
    f.write("source/kitchen/changed.txt", b"changed");
    let roots = vec![f.0.join("source")];
    let internal = vec![f.0.join("source/kitchen/catalog")];
    std::fs::create_dir_all(&internal[0]).unwrap();
    let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
        .add_path(f.0.join("source/kitchen/changed.txt"))
        .add_path(f.0.join("outside/not-allowed.txt"))
        .add_path(internal[0].join("database.sqlite"))
        .add_path(f.0.join("source/kitchen/../kitchen-old/file.txt"));
    assert_eq!(event_scopes(&event, &roots, &internal), [f.0.join("source/kitchen")]);
    let mut pending = PendingScan::default();
    coalesce_path(&mut pending, f.0.join("source/kitchen/subfolder"));
    coalesce_path(&mut pending, f.0.join("source/kitchen-old"));
    coalesce_path(&mut pending, f.0.join("source/kitchen"));
    assert_eq!(pending.paths.len(), 2);
    for i in 0..70 { coalesce_path(&mut pending, f.0.join(format!("source/folder-{i}"))); }
    assert!(pending.full);
    assert!(pending.paths.is_empty());
}

#[tokio::test]
async fn native_file_notification_drives_actual_subtree_indexing() {
    let f = Fixture::new();
    let (db, id) = f.open().await;
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(32);
    let mut watcher = notify::recommended_watcher(move |event| { let _ = sender.try_send(event); }).unwrap();
    watcher.watch(&f.0.join("source"), RecursiveMode::Recursive).unwrap();
    f.write("source/kitchen/notified.txt", b"native notification");
    let event = tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let event: notify::Result<Event> = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
            let event = event.unwrap();
            if !matches!(event.kind, EventKind::Access(_)) && !event.paths.is_empty() { return event; }
        }
    }).await.unwrap();
    let scopes = event_scopes(&event, &[f.0.join("source")], &[]);
    let scans: Vec<_> = scopes.iter().map(|path| path.to_string_lossy().into_owned()).collect();
    index_paths(&db, &id, &scans).await.unwrap();
    assert_eq!(paths(&db, &id).await, ["kitchen/notified.txt"]);
    drop(watcher);
    close(db).await;
}

#[tokio::test]
async fn partial_scan_preserves_last_full_scan_date() {
    let f = Fixture::new();
    let (db, id) = f.open().await;
    index_paths(&db, &id, &[f.path("source")]).await.unwrap();
    sqlx::query("UPDATE settings SET value='123' WHERE key=?").bind(format!("last_scan:{id}")).execute(&db.master_pool).await.unwrap();
    index_paths(&db, &id, &[f.path("source/kitchen")]).await.unwrap();
    let recorded: String = sqlx::query_scalar("SELECT value FROM settings WHERE key=?").bind(format!("last_scan:{id}")).fetch_one(&db.master_pool).await.unwrap();
    assert_eq!(recorded,"123");
    close(db).await;
}
