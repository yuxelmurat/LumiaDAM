use super::*;
use crate::db::{connect_library_sqlite, Database};

#[test]
fn shared_thumbnails_are_legacy_but_asset_specific_thumbnails_are_safe() {
    assert!(crate::db::is_legacy_thumbnail(r"C:\LumiaDAM\thumbs\9_thumb.jpg"));
    assert!(!crate::db::is_legacy_thumbnail(r"C:\LumiaDAM\thumbs\asset-id\9_thumb.jpg"));
}

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

async fn search_catalog(db: &Database, query: serde_json::Value, page_size: i64) -> crate::models::SearchResult {
    let mut value = serde_json::json!({"text":"", "filter":{}, "sort":{"field":"name","order":"asc"}});
    value.as_object_mut().unwrap().extend(query.as_object().unwrap().clone());
    crate::commands::asset::search_catalogs(crate::models::SearchPayload {
        query: serde_json::from_value(value).unwrap(), page: 0, page_size, result_scope: None,
    }, db).await.unwrap()
}

#[tokio::test]
async fn old_hdr_errors_are_retried_once_when_catalog_opens() {
    let fixture = Fixture::new();
    let source = fixture.0.join("images");
    std::fs::create_dir(&source).unwrap();
    for name in ["light.hdr", "room.exr", "model.max"] {
        std::fs::write(source.join(name), b"test").unwrap();
    }
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    db.register_library(&id, "Images", &source).await.unwrap();
    crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.unwrap();
    let library = db.get_library_handle(Some(&id)).await.unwrap();
    sqlx::query("UPDATE preview_jobs SET status = 'error'").execute(&library.pool).await.unwrap();
    sqlx::query("UPDATE assets SET preview_status = 'error'").execute(&library.pool).await.unwrap();

    crate::preview::retry_failed_hdr_previews(&library.pool).await.unwrap();
    let states: Vec<(String, String)> = sqlx::query_as(
        "SELECT a.extension, p.status FROM assets a JOIN preview_jobs p ON p.asset_id = a.id ORDER BY a.extension"
    ).fetch_all(&library.pool).await.unwrap();
    assert_eq!(states, [("exr".into(), "queued".into()), ("hdr".into(), "queued".into()), ("max".into(), "error".into())]);
    sqlx::query("UPDATE preview_jobs SET status = 'error' WHERE asset_id IN (SELECT id FROM assets WHERE extension = 'hdr')")
        .execute(&library.pool).await.unwrap();
    crate::preview::retry_failed_hdr_previews(&library.pool).await.unwrap();
    let hdr_status: String = sqlx::query_scalar("SELECT p.status FROM preview_jobs p JOIN assets a ON a.id = p.asset_id WHERE a.extension = 'hdr'")
        .fetch_one(&library.pool).await.unwrap();
    assert_eq!(hdr_status, "error");
    close(db).await;
}

#[tokio::test]
async fn related_images_match_indexed_siblings_without_crossing_folders_or_filters() {
    let fixture = Fixture::new();
    let source = fixture.0.join("archive");
    for folder in ["kitchen", "other"] { std::fs::create_dir_all(source.join(folder)).unwrap(); }
    for file in [
        "kitchen/Kitchen123.max", "kitchen/Kitchen123.zip", "kitchen/Kitchen123.jpg", "other/Kitchen123.jpg",
        "kitchen/Chair.max", "kitchen/Chair_preview.png", "kitchen/Chair_diffuse.jpg",
        "kitchen/Desk.max", "kitchen/Desk_diffuse.jpg", "other/Desk.jpg",
        "kitchen/Lamp.max", "kitchen/lamp.JPG",
    ] { std::fs::write(source.join(file), b"test").unwrap(); }
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    db.register_library(&id, "Archive", &source).await.unwrap();
    crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.unwrap();
    std::fs::rename(&source, fixture.0.join("offline")).unwrap();

    let result = search_catalog(&db, serde_json::json!({"filter":{"kinds":["3d_model", "other"]}}), 100).await;
    let find = |name: &str| result.assets.iter().find(|asset| asset.file_name == name).unwrap();
    for name in ["Kitchen123.max", "Kitchen123.zip"] {
        let related = find(name).related_image.as_ref().unwrap();
        assert_eq!(related.file_name, "Kitchen123.jpg");
        assert!(related.file_path.replace('\\', "/").ends_with("archive/kitchen/Kitchen123.jpg"));
    }
    assert_eq!(find("Chair.max").related_image.as_ref().unwrap().file_name, "Chair_preview.png");
    assert_eq!(find("Lamp.max").related_image.as_ref().unwrap().file_name, "lamp.JPG");
    assert!(find("Desk.max").related_image.is_none());
    let old_thumb = fixture.0.join("local-libraries/thumbs/Kitchen123_thumb.jpg");
    let library = db.get_library_handle(Some(&id)).await.unwrap();
    sqlx::query("UPDATE assets SET thumbnail_path = ? WHERE file_name = 'Kitchen123.max'")
        .bind(old_thumb.to_string_lossy().as_ref()).execute(&library.pool).await.unwrap();
    sqlx::query("UPDATE assets SET thumbnail_path = ? WHERE file_name = 'Kitchen123.jpg'")
        .bind(old_thumb.to_string_lossy().as_ref()).execute(&library.pool).await.unwrap();
    let refreshed = search_catalog(&db, serde_json::json!({"text":"Kitchen123.max"}), 100).await;
    assert!(refreshed.assets[0].thumbnail_path.is_none());
    assert!(refreshed.assets[0].related_image.is_some());
    let images = search_catalog(&db, serde_json::json!({"text":"Kitchen123.jpg"}), 100).await;
    assert_eq!(images.assets.len(), 2);
    assert!(images.assets.iter().all(|image| image.thumbnail_path.is_none()));
    let archive = search_catalog(&db, serde_json::json!({"text":"Kitchen123.zip"}), 100).await;
    let related = archive.assets[0].related_image.as_ref().unwrap();
    assert!(related.file_path.replace('\\', "/").ends_with("archive/kitchen/Kitchen123.jpg"));
    assert!(related.thumbnail_path.is_none());
    close(db).await;
}

#[tokio::test]
async fn folder_search_browses_numeric_archives_offline_with_exact_boundaries() {
    let fixture = Fixture::new();
    let source = fixture.0.join("Model Archive");
    for folder in ["2023/3dsky/furniture/kitchen/corona", "2023/3dsky/furniture/kitchen/vray", "kitchen-old", "empty-kitchen", "100%_kitchen"] {
        std::fs::create_dir_all(source.join(folder)).unwrap();
    }
    for file in ["2023/3dsky/furniture/kitchen/corona/2542154.454545.zip", "2023/3dsky/furniture/kitchen/vray/555.rar", "kitchen-old/999.7z", "kitchen.jpg"] {
        std::fs::write(source.join(file), b"test").unwrap();
    }
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    db.register_library(&id, "Models", &source).await.unwrap();
    crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.unwrap();
    std::fs::rename(&source, fixture.0.join("offline")).unwrap();
    let result = search_catalog(&db, serde_json::json!({"text":"kitchen"}), 100).await;
    assert_eq!(result.assets.len(), 4);
    assert!(result.assets.iter().any(|asset| asset.file_name == "kitchen.jpg"));
    assert_eq!(result.folder_total, 6);
    let kitchen = result.folders.iter().find(|folder| folder.name == "kitchen").unwrap();
    assert!(kitchen.full_path.replace('\\', "/").ends_with("Model Archive/2023/3dsky/furniture/kitchen"));
    let inside = search_catalog(&db, serde_json::json!({"libraryId":id,"folderPath":kitchen.path}), 100).await;
    assert!(inside.assets.is_empty());
    assert_eq!(inside.folder_total, 2);
    let corona = search_catalog(&db, serde_json::json!({"libraryId":id,"folderPath":"2023/3dsky/furniture/kitchen/corona"}), 100).await;
    assert_eq!(corona.assets.len(), 1);
    assert_eq!(corona.assets[0].extension, "zip");
    assert!(corona.assets[0].file_path.replace('\\', "/").contains("3dsky/furniture/kitchen/corona/"));
    assert_eq!(corona.folder_total, 0);
    let literal = search_catalog(&db, serde_json::json!({"text":"100%_kitchen"}), 100).await;
    assert_eq!(literal.folder_total, 1);
    let root = search_catalog(&db, serde_json::json!({"libraryId":id,"folderPath":""}), 100).await;
    assert_eq!(root.assets.len(), 1);
    assert_eq!(root.folder_total, 4);
    let root_match = search_catalog(&db, serde_json::json!({"text":"Model Archive"}), 100).await;
    assert!(root_match.folders.iter().any(|folder| folder.path.is_empty()));
    assert_eq!(search_catalog(&db, serde_json::json!({"text":"kitchen","libraryId":"missing"}), 100).await.folder_total, 0);
    close(db).await;
}

#[tokio::test]
async fn folders_paginate_across_libraries_and_respect_selected_scope() {
    let fixture = Fixture::new();
    let db = fixture.database().await;
    let mut ids = Vec::new();
    for name in ["first", "second"] {
        let root = fixture.0.join(name);
        std::fs::create_dir_all(root.join("kitchen")).unwrap();
        let id = Uuid::new_v4().to_string();
        register_row(&db, &id, &root).await;
        db.register_library(&id, name, &root).await.unwrap();
        crate::watcher::index_paths(&db, &id, &[root.to_string_lossy().into_owned()]).await.unwrap();
        ids.push(id);
    }
    let first = search_catalog(&db, serde_json::json!({"text":"KITCHEN"}), 1).await;
    let second = search_catalog(&db, serde_json::json!({"text":"KITCHEN","folderPage":1}), 1).await;
    assert_eq!(first.folder_total, 2);
    assert_eq!(first.folders.len(), 1);
    assert_ne!(first.folders[0].library_id, second.folders[0].library_id);
    let selected = search_catalog(&db, serde_json::json!({"text":"kitchen","libraryIds":[ids[0]]}), 100).await;
    assert_eq!(selected.folder_total, 1);
    assert_eq!(selected.folders[0].library_id, ids[0]);
    close(db).await;
}

#[tokio::test]
async fn old_catalog_backfills_parent_folders_without_accessing_source() {
    let fixture = Fixture::new();
    let path = fixture.0.join("legacy.sqlite");
    let pool = connect_library_sqlite(&path).await.unwrap();
    sqlx::query("DROP TABLE folders; INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES('asset','2023/kitchen/corona/123.jpg','123.jpg','jpg','texture',1,1,1)")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let pool = connect_library_sqlite(&path).await.unwrap();
    let paths: Vec<String> = sqlx::query_scalar("SELECT path FROM folders ORDER BY path").fetch_all(&pool).await.unwrap();
    assert_eq!(paths, vec!["2023", "2023/kitchen", "2023/kitchen/corona"]);
    pool.close().await;
}

#[tokio::test]
async fn search_combines_paths_and_names_with_filters_and_indexes_all_file_types() {
    let fixture = Fixture::new();
    let source = fixture.0.join("Models");
    let files = ["kitchen/corona/123.jpg", "kitchen/456.png", "kitchen/789.xyz", "kitchen/README",
        "kitchen/.notes", "kitchen/backup.0001.rfa", "kitchen-only-zip/123.zip", "living/kitchenxxxxx.jpg", "living/other.jpg"];
    for file in files {
        let path = source.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"test").unwrap();
    }
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    db.register_library(&id, "Models", &source).await.unwrap();
    crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.unwrap();
    let all = search_catalog(&db, serde_json::json!({}), 100).await;
    assert_eq!(all.total, files.len() as i64);
    let jpg = search_catalog(&db, serde_json::json!({"text":"kitchen", "filter":{"extensions":["jpg"]}}), 100).await;
    assert_eq!(jpg.total, 2);
    assert!(jpg.assets.iter().any(|asset| asset.file_name == "123.jpg"));
    assert!(jpg.assets.iter().any(|asset| asset.file_name == "kitchenxxxxx.jpg"));
    assert_eq!(jpg.folder_total, 2);
    assert!(jpg.folders.iter().any(|folder| folder.path == "kitchen"));
    assert!(!jpg.folders.iter().any(|folder| folder.path == "kitchen-only-zip"));
    let texture = search_catalog(&db, serde_json::json!({"text":"kitchen", "filter":{"kinds":["texture"]}}), 100).await;
    assert_eq!(texture.total, 3);
    let multi = search_catalog(&db, serde_json::json!({"text":"kitchen corona", "filter":{"extensions":["jpg"]}}), 100).await;
    assert_eq!(multi.total, 1);
    assert_eq!(multi.folder_total, 1);
    let opened = search_catalog(&db, serde_json::json!({"libraryId":id,"folderPath":"kitchen", "filter":{"extensions":["jpg"]}}), 100).await;
    assert_eq!(opened.total, 0);
    assert_eq!(opened.folder_total, 1);
    let unfiltered = search_catalog(&db, serde_json::json!({"libraryId":id,"folderPath":"kitchen"}), 100).await;
    assert_eq!(unfiltered.total, 5);
    // Klasör görünürlüğünde puan gibi diğer filtreler de aynı dosya koşullarını kullanır.
    let rated = search_catalog(&db, serde_json::json!({"text":"kitchen","filter":{"rating":{"min":5,"max":5}}}), 100).await;
    assert_eq!(rated.total, 0);
    assert_eq!(rated.folder_total, 0);
    // Kütüphane kökü de tam yol aramasına dahildir.
    let root = search_catalog(&db, serde_json::json!({"text":"Models", "filter":{"extensions":["jpg"]}}), 100).await;
    assert_eq!(root.total, 3);
    close(db).await;
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
    // WAL kapanışının arka plandaki checkpoint'i kaynak bayt ölçümünü etkilemesin.
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool).await.unwrap();
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
    let after = std::fs::read(legacy.join("library.sqlite")).unwrap();
    assert!(after == original, "source bytes changed: before={} after={}, first difference={:?}, before header={:?}, after header={:?}", original.len(), after.len(), original.iter().zip(&after).position(|(a,b)| a != b), &original[..100], &after[..100]);
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
