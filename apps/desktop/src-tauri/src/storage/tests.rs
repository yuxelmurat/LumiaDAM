use super::*;
use crate::db::{connect_library_sqlite, Database};

#[test]
fn library_names_use_folder_or_drive_and_preserve_custom_names() {
    use crate::db::library_name;
    assert_eq!(library_name("", "A:\\"), "A:");
    assert_eq!(library_name("New Library", "A:/"), "A:");
    assert_eq!(library_name("", "D:\\Model Archive\\"), "Model Archive");
    assert_eq!(library_name("", "\\\\nas\\Models\\"), "Models");
    assert_eq!(library_name("", "/mnt/models/"), "models");
    assert_eq!(library_name("", "/"), "/");
    assert_eq!(library_name("My Library", "A:\\"), "My Library");
}

#[tokio::test]
async fn opening_catalog_repairs_placeholder_name_without_rescanning() {
    let fixture = Fixture::new();
    let source = fixture.0.join("Models");
    std::fs::create_dir(&source).unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    db.register_library(&id, "New Library", &source).await.unwrap();
    sqlx::query("UPDATE libraries SET name = 'New Library'").execute(&db.master_pool).await.unwrap();
    close(db).await;
    std::fs::remove_dir(&source).unwrap();
    let db = fixture.database().await;
    let name: String = sqlx::query_scalar("SELECT name FROM libraries WHERE id = ?")
        .bind(&id).fetch_one(&db.master_pool).await.unwrap();
    assert_eq!(name, "Models");
    assert_eq!(db.get_library_handle(Some(&id)).await.unwrap().name, "Models");
    close(db).await;
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lumiadam-storage-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    async fn database(&self) -> Database {
        Database::new(self.0.join("master.sqlite").to_str().unwrap(), self.0.join("local-libraries")).await.unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

async fn close(db: Database) {
    for library in db.get_all_libraries().await { library.pool.close().await; }
    db.master_pool.close().await;
}

async fn register_row(db: &Database, id: &str, source: &Path) {
    sqlx::query("INSERT INTO libraries(id, name, root_paths, created_at) VALUES(?, 'Arşiv', ?, 1)")
        .bind(id).bind(serde_json::to_string(&vec![source]).unwrap())
        .execute(&db.master_pool).await.unwrap();
}

#[tokio::test]
async fn legacy_migration_preserves_metadata_previews_and_source() {
    let fixture = Fixture::new();
    let source = fixture.0.join("nas");
    let legacy = source.join(".opendam");
    std::fs::create_dir_all(legacy.join("thumbs")).unwrap();
    std::fs::write(legacy.join("thumbs/123_thumb.jpg"), b"preview").unwrap();
    std::fs::write(legacy.join("thumbs/123.glb"), b"model").unwrap();
    let pool = connect_library_sqlite(&legacy.join("library.sqlite")).await.unwrap();
    sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at,rating,thumbnail_path,metadata) VALUES('asset','2023/mutfak/123.jpg','123.jpg','jpg','texture',10,1,1,5,'.opendam/thumbs/123_thumb.jpg','{\"preview_model\":\".opendam/thumbs/123.glb\"}')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO tags(id,name) VALUES('tag','Mutfak'); INSERT INTO asset_tags VALUES('asset','tag'); INSERT INTO collections(id,name,created_at) VALUES('collection','Favoriler',1); INSERT INTO collection_assets VALUES('collection','asset');")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let original = std::fs::read(legacy.join("library.sqlite")).unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    let handle = db.register_library(&id, "Arşiv", &source).await.unwrap();
    assert!(handle.catalog_dir.starts_with(&db.storage_root));
    let (rating, thumbnail, metadata): (i64, String, String) = sqlx::query_as("SELECT rating,thumbnail_path,metadata FROM assets WHERE id='asset'")
        .fetch_one(&handle.pool).await.unwrap();
    assert_eq!(rating, 5);
    assert!(Path::new(&thumbnail).starts_with(&handle.catalog_dir));
    assert_eq!(std::fs::read(thumbnail).unwrap(), b"preview");
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(std::fs::read(metadata["preview_model"].as_str().unwrap()).unwrap(), b"model");
    for table in ["asset_tags", "collection_assets"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", table)).fetch_one(&handle.pool).await.unwrap();
        assert_eq!(count, 1);
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM assets_fts WHERE assets_fts MATCH '123*'").fetch_one(&handle.pool).await.unwrap();
    assert_eq!(count, 1);
    assert_eq!(std::fs::read(legacy.join("library.sqlite")).unwrap(), original);
    assert!(!legacy.join("index.lock").exists());
    close(db).await;
}

#[tokio::test]
async fn local_catalog_reopens_offline_and_failed_scan_does_not_delete_assets() {
    let fixture = Fixture::new();
    let source = fixture.0.join("nas");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("asset.jpg"), b"source").unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    let handle = db.register_library(&id, "Arşiv", &source).await.unwrap();
    crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.unwrap();
    assert!(!source.join(".opendam").exists());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM assets").fetch_one(&handle.pool).await.unwrap();
    assert_eq!(count, 1);
    close(db).await;
    std::fs::rename(&source, fixture.0.join("disconnected")).unwrap();
    let db = fixture.database().await;
    assert!(db.storage_errors.is_empty());
    let handle = db.get_library_handle(Some(&id)).await.unwrap();
    assert!(crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM assets").fetch_one(&handle.pool).await.unwrap();
    assert_eq!(count, 1);
    assert!(db.get_library_handle(Some("missing-id")).await.is_none());
    close(db).await;
}

#[tokio::test]
async fn location_change_copies_latest_local_data_without_overwriting_old_copy() {
    let fixture = Fixture::new();
    let source = fixture.0.join("nas");
    std::fs::create_dir(&source).unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    let handle = db.register_library(&id, "Arşiv", &source).await.unwrap();
    let old_dir = handle.catalog_dir.clone();
    std::fs::write(old_dir.join("thumbs/test.jpg"), b"local preview").unwrap();
    sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at,thumbnail_path) VALUES('asset','test.jpg','test.jpg','jpg','texture',1,1,1,?)")
        .bind(old_dir.join("thumbs/test.jpg").to_string_lossy().as_ref()).execute(&handle.pool).await.unwrap();
    let selected = fixture.0.join("selected-storage");
    sqlx::query("INSERT INTO settings VALUES('storage_root', ?)").bind(selected.to_string_lossy().as_ref()).execute(&db.master_pool).await.unwrap();
    close(db).await;
    std::fs::rename(&source, fixture.0.join("offline")).unwrap();
    let db = fixture.database().await;
    assert!(db.storage_errors.is_empty());
    let new_handle = db.get_library_handle(Some(&id)).await.unwrap();
    assert_eq!(new_handle.catalog_dir.parent(), Some(selected.as_path()));
    let thumbnail: String = sqlx::query_scalar("SELECT thumbnail_path FROM assets").fetch_one(&new_handle.pool).await.unwrap();
    assert!(Path::new(&thumbnail).starts_with(&new_handle.catalog_dir));
    assert_eq!(std::fs::read(thumbnail).unwrap(), b"local preview");
    assert!(old_dir.join("library.sqlite").exists());
    close(db).await;
}

#[tokio::test]
async fn failed_migration_does_not_publish_partial_catalog_or_replace_source() {
    let fixture = Fixture::new();
    let source = fixture.0.join("broken");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("library.sqlite"), b"not a database").unwrap();
    let target = fixture.0.join("local");
    assert!(prepare_catalog(&target, "Arşiv", &Uuid::new_v4().to_string(), Some(&source)).await.is_err());
    assert_eq!(std::fs::read_dir(target).unwrap().count(), 0);
    assert_eq!(std::fs::read(source.join("library.sqlite")).unwrap(), b"not a database");
}

#[tokio::test]
async fn missing_registered_catalog_is_not_replaced_by_an_empty_database() {
    let fixture = Fixture::new();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &fixture.0.join("offline")).await;
    let missing = fixture.0.join("missing-catalog");
    sqlx::query("UPDATE libraries SET catalog_path = ?").bind(missing.to_string_lossy().as_ref()).execute(&db.master_pool).await.unwrap();
    close(db).await;
    let db = fixture.database().await;
    assert_eq!(db.storage_errors.len(), 1);
    assert!(db.get_all_libraries().await.is_empty());
    assert!(!missing.exists());
    close(db).await;
}

#[test]
fn same_names_are_distinct_and_unsafe_storage_paths_are_rejected() {
    let first = catalog_name("../../Arşiv:C", &Uuid::new_v4().to_string()).unwrap();
    let second = catalog_name("../../Arşiv:C", &Uuid::new_v4().to_string()).unwrap();
    assert_ne!(first, second);
    assert!(!first.contains('/') && !first.contains(':'));
    assert!(catalog_name("test", "../../invalid").is_err());
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    let nested = source.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    assert!(validate_location(&nested, std::slice::from_ref(&source), &[]).is_err());
    assert!(validate_location(Path::new("relative"), &[], &[]).is_err());
    assert!(validate_location(&fixture.0, &[], &[]).is_ok());
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[tokio::test]
async fn catalogs_inside_a_source_drive_are_not_indexed_as_assets() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("source.jpg"), b"source").unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &fixture.0).await;
    let handle = db.register_library(&id, "Sürücü", &fixture.0).await.unwrap();
    std::fs::write(handle.catalog_dir.join("thumbs/cached.jpg"), b"preview").unwrap();
    let old_storage = fixture.0.join("old-storage");
    std::fs::create_dir(&old_storage).unwrap();
    std::fs::write(old_storage.join(".lumiadam-catalog"), b"old catalog").unwrap();
    std::fs::write(old_storage.join("cached.jpg"), b"preview").unwrap();
    crate::watcher::index_paths(&db, &id, &[fixture.0.to_string_lossy().into_owned()]).await.unwrap();
    let names: Vec<String> = sqlx::query_scalar("SELECT file_name FROM assets").fetch_all(&handle.pool).await.unwrap();
    assert_eq!(names, vec!["source.jpg"]);
    close(db).await;
}
