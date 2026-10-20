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

#[tokio::test]
async fn opening_offline_catalog_repairs_zero_asset_count_from_local_database() {
    let fixture = Fixture::new();
    let source = fixture.0.join("Models");
    std::fs::create_dir(&source).unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    let library = db.register_library(&id, "Models", &source).await.unwrap();
    for n in 0..4 {
        sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES(?,?,?,'jpg','image',1,1,1)")
            .bind(format!("asset-{n}")).bind(format!("folder/{n}.jpg")).bind(format!("{n}.jpg"))
            .execute(&library.pool).await.unwrap();
    }
    sqlx::query("UPDATE libraries SET asset_count=0 WHERE id=?").bind(&id).execute(&db.master_pool).await.unwrap();
    close(db).await;
    std::fs::remove_dir(&source).unwrap();

    let db = fixture.database().await;
    let count: i64 = sqlx::query_scalar("SELECT asset_count FROM libraries WHERE id=?").bind(&id).fetch_one(&db.master_pool).await.unwrap();
    assert_eq!(count, 4, "count repair must use the local catalog even while the source is offline");
    close(db).await;
}

#[tokio::test]
async fn asset_collection_lookup_uses_reverse_index() {
    let fixture = Fixture::new();
    let source = fixture.0.join("Models");
    std::fs::create_dir(&source).unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    let library = db.register_library(&id, "Models", &source).await.unwrap();

    let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT collection_id FROM collection_assets WHERE asset_id = 'asset'",
    )
    .fetch_all(&library.pool)
    .await
    .unwrap();

    assert!(plan.iter().any(|(_, _, _, detail)| detail.contains("idx_collection_assets_asset")), "unexpected query plan: {plan:?}");
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

#[tokio::test]
async fn folder_labels_and_grouped_pages_work_offline_without_crossing_directories() {
    let fixture=Fixture::new();let source=fixture.0.join("NAS");
    std::fs::create_dir_all(source.join("2023/3dsky/kitchen/corona")).unwrap();
    std::fs::create_dir_all(source.join("2025/bedroom")).unwrap();
    for path in ["2023/3dsky/kitchen/corona/9.zip","2023/3dsky/kitchen/corona/9.jpg","2023/3dsky/kitchen/corona/8.zip","2023/3dsky/kitchen/corona/8_render.jpg","2025/bedroom/9.jpg"] {
        std::fs::write(source.join(path),path.as_bytes()).unwrap();
    }
    let db=fixture.database().await;let id=Uuid::new_v4().to_string();register_row(&db,&id,&source).await;
    let lib=db.register_library(&id,"NAS",&source).await.unwrap();
    crate::watcher::index_paths(&db,&id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    std::fs::rename(&source,fixture.0.join("disconnected")).unwrap();
    let query=serde_json::json!({"groupRelated":true,"filter":{"pathLabels":{"year":"2023","source":"3dsky","room":"Kitchen","renderer":"Corona"}}});
    let grouped=search_catalog(&db,query.clone(),1).await;
    assert_eq!(grouped.total,2);assert_eq!(grouped.assets.len(),1);
    assert_eq!(grouped.assets[0].extension,"zip");assert_eq!(grouped.assets[0].group_files.len(),2);
    assert!(grouped.assets[0].group_files.iter().all(|file|file.file_path.contains("kitchen")));
    // Açılış sorgusu da filtreli gruplayıcıyla aynı grupları ve kararlı sayfaları döndürmeli.
    let all=search_catalog(&db,serde_json::json!({"groupRelated":true}),100).await;
    assert_eq!(all.total,3);assert_eq!(all.assets.len(),3);
    assert_eq!(all.assets.iter().filter(|asset|asset.extension=="zip").count(),2);
    assert_eq!(all.assets.iter().filter(|asset|asset.extension=="jpg").count(),1);
    let first=search_catalog(&db,serde_json::json!({"groupRelated":true}),1).await;
    let unfiltered_next=crate::models::SearchPayload {query:serde_json::from_value(serde_json::json!({"text":"","groupRelated":true,"filter":{},"sort":{"field":"name","order":"asc"}})).unwrap(),page:1,page_size:1,result_scope:Some("files".into())};
    let next_page=crate::commands::asset::search_catalogs(unfiltered_next,&db).await.unwrap();
    assert_eq!(first.assets[0].id,all.assets[0].id);assert_eq!(next_page.assets[0].id,all.assets[1].id);
    let next=crate::models::SearchPayload {query:serde_json::from_value(serde_json::json!({"text":"","groupRelated":true,"filter":query["filter"],"sort":{"field":"name","order":"asc"}})).unwrap(),page:1,page_size:1,result_scope:Some("files".into())};
    let second=crate::commands::asset::search_catalogs(next,&db).await.unwrap();
    assert_ne!(grouped.assets[0].id,second.assets[0].id);
    let images=search_catalog(&db,serde_json::json!({"groupRelated":true,"filter":{"extensions":["jpg"],"pathLabels":{"room":"Kitchen","year":"","source":"","renderer":""}}}),100).await;
    assert_eq!(images.total,2);assert!(images.assets.iter().all(|asset|asset.extension=="jpg"));
    sqlx::query("UPDATE assets SET is_favorite=1 WHERE extension='jpg'").execute(&lib.pool).await.unwrap();
    let favorites=search_catalog(&db,serde_json::json!({"groupRelated":true,"filter":{"isFavorite":true,"pathLabels":{"room":"Kitchen","year":"","source":"","renderer":""}}}),100).await;
    assert_eq!(favorites.total,2);assert!(favorites.assets.iter().all(|asset|asset.is_favorite && asset.extension=="jpg"));
    close(db).await;
}

#[tokio::test]
async fn user_label_corrections_survive_changed_source_and_restore_to_inferred_values() {
    let fixture=Fixture::new();let source=fixture.0.join("NAS");std::fs::create_dir_all(source.join("2023/kitchen/corona")).unwrap();
    let path=source.join("2023/kitchen/corona/9.zip");std::fs::write(&path,b"archive").unwrap();
    let db=fixture.database().await;let library_id=Uuid::new_v4().to_string();register_row(&db,&library_id,&source).await;
    let lib=db.register_library(&library_id,"NAS",&source).await.unwrap();
    crate::watcher::index_paths(&db,&library_id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    let id:String=sqlx::query_scalar("SELECT id FROM assets").fetch_one(&lib.pool).await.unwrap();
    sqlx::query("UPDATE asset_workflow SET overrides=? WHERE asset_id=?").bind(serde_json::json!({"room":"Office"}).to_string())
        .bind(&id).execute(&lib.pool).await.unwrap();
    std::fs::write(&path,b"changed archive contents").unwrap();
    crate::watcher::index_paths(&db,&library_id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    let room:String=sqlx::query_scalar("SELECT room FROM asset_workflow WHERE asset_id=?").bind(&id).fetch_one(&lib.pool).await.unwrap();assert_eq!(room,"Office");
    sqlx::query("UPDATE asset_workflow SET overrides='{}' WHERE asset_id=?").bind(&id).execute(&lib.pool).await.unwrap();
    let mut connection=lib.pool.acquire().await.unwrap();crate::workflow::upsert(&mut connection,&id,"2023/kitchen/corona/9.zip","zip","other").await.unwrap();drop(connection);
    let asset=search_catalog(&db,serde_json::json!({}),100).await.assets.remove(0);assert_eq!(asset.path_labels.room,"Kitchen");
    assert_eq!(asset.inferred_labels.renderer,"Corona");close(db).await;
}

#[tokio::test]
async fn full_offline_copy_reopens_without_source_and_survives_storage_transfer() {
    let fixture=Fixture::new();let source=fixture.0.join("NAS");std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("9.zip"),b"usable archive contents").unwrap();
    let db=fixture.database().await;let library_id=Uuid::new_v4().to_string();register_row(&db,&library_id,&source).await;
    let lib=db.register_library(&library_id,"NAS",&source).await.unwrap();
    crate::watcher::index_paths(&db,&library_id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    let id:String=sqlx::query_scalar("SELECT id FROM assets").fetch_one(&lib.pool).await.unwrap();
    crate::commands::workflow::cache_file(&db,id.clone(),None).await.unwrap();
    let export_id=Uuid::new_v4().to_string();let exports=lib.catalog_dir.join("exports");std::fs::create_dir(&exports).unwrap();let export_path=exports.join("render.png");std::fs::write(&export_path,b"local render").unwrap();
    sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES(?,?,'render.png','png','image',12,1,1)").bind(&export_id).bind(export_path.to_string_lossy().as_ref()).execute(&lib.pool).await.unwrap();
    crate::workflow::initialize(&lib.pool).await.unwrap();
    let cache=crate::workflow::readable_path(&db,&id).await.unwrap();assert!(cache.starts_with(lib.catalog_dir.canonicalize().unwrap()));
    assert!(std::fs::metadata(&cache).unwrap().permissions().readonly());
    std::fs::rename(&source,fixture.0.join("disconnected")).unwrap();
    assert!(crate::commands::workflow::cache_file(&db,id.clone(),None).await.is_err());
    assert_eq!(std::fs::read(&cache).unwrap(),b"usable archive contents");
    assert_eq!(std::fs::read(crate::workflow::readable_path(&db,&export_id).await.unwrap()).unwrap(),b"local render");
    assert_eq!(std::fs::read(crate::workflow::readable_path(&db,&id).await.unwrap()).unwrap(),b"usable archive contents");
    let target=fixture.0.join("new-storage");std::fs::create_dir(&target).unwrap();
    sqlx::query("INSERT INTO settings(key,value) VALUES('storage_root',?)").bind(target.to_string_lossy().as_ref()).execute(&db.master_pool).await.unwrap();
    close(db).await;
    let db=fixture.database().await;let cache=crate::workflow::readable_path(&db,&id).await.unwrap();
    assert!(cache.starts_with(target.canonicalize().unwrap()));assert_eq!(std::fs::read(cache).unwrap(),b"usable archive contents");
    let export=crate::workflow::readable_path(&db,&export_id).await.unwrap();assert!(export.starts_with(target.canonicalize().unwrap()));assert_eq!(std::fs::read(export).unwrap(),b"local render");
    let asset=search_catalog(&db,serde_json::json!({"filter":{"offlineOnly":true}}),100).await.assets.remove(0);assert!(asset.offline_path.is_some());assert!(!asset.offline_stale);
    close(db).await;
}

#[tokio::test]
async fn render_pin_rejects_other_folders_and_automatic_matching_can_be_restored() {
    let fixture=Fixture::new();let source=fixture.0.join("NAS");std::fs::create_dir_all(source.join("kitchen")).unwrap();std::fs::create_dir_all(source.join("bedroom")).unwrap();
    for path in ["kitchen/9.zip","kitchen/9.jpg","kitchen/8.jpg","bedroom/8.jpg"] {std::fs::write(source.join(path),b"asset").unwrap();}
    let db=fixture.database().await;let library_id=Uuid::new_v4().to_string();register_row(&db,&library_id,&source).await;
    let lib=db.register_library(&library_id,"NAS",&source).await.unwrap();crate::watcher::index_paths(&db,&library_id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT file_path,id FROM assets").fetch_all(&lib.pool).await.unwrap();let ids:std::collections::HashMap<_,_>=rows.into_iter().collect();
    let zip=ids["kitchen/9.zip"].clone();
    assert!(crate::commands::workflow::link_image(&db,zip.clone(),Some(ids["bedroom/8.jpg"].clone()),"pinned".into()).await.is_err());
    assert!(crate::commands::workflow::link_image(&db,zip.clone(),Some(zip.clone()),"pinned".into()).await.is_err());
    crate::commands::workflow::link_image(&db,zip.clone(),Some(ids["kitchen/8.jpg"].clone()),"pinned".into()).await.unwrap();
    let query=serde_json::json!({"filter":{"extensions":["zip"]}});
    let asset=search_catalog(&db,query.clone(),100).await.assets.remove(0);assert_eq!(asset.related_mode,"pinned");assert_eq!(asset.related_image.unwrap().id,ids["kitchen/8.jpg"]);
    crate::commands::workflow::link_image(&db,zip.clone(),None,"none".into()).await.unwrap();assert!(search_catalog(&db,query.clone(),100).await.assets[0].related_image.is_none());
    crate::commands::workflow::link_image(&db,zip,None,"auto".into()).await.unwrap();assert_eq!(search_catalog(&db,query,100).await.assets[0].related_image.as_ref().unwrap().id,ids["kitchen/9.jpg"]);close(db).await;
}

#[tokio::test]
async fn outdated_offline_copy_is_reported_and_not_opened_as_the_current_version() {
    let fixture=Fixture::new();let source=fixture.0.join("NAS");std::fs::create_dir(&source).unwrap();std::fs::write(source.join("9.max"),b"old").unwrap();
    let db=fixture.database().await;let library_id=Uuid::new_v4().to_string();register_row(&db,&library_id,&source).await;
    let lib=db.register_library(&library_id,"NAS",&source).await.unwrap();crate::watcher::index_paths(&db,&library_id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    let id:String=sqlx::query_scalar("SELECT id FROM assets").fetch_one(&lib.pool).await.unwrap();
    crate::commands::workflow::cache_file(&db,id.clone(),None).await.unwrap();
    std::fs::write(source.join("9.max"),b"new scene version").unwrap();crate::watcher::index_paths(&db,&library_id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    let asset=search_catalog(&db,serde_json::json!({}),100).await.assets.remove(0);assert!(asset.offline_stale);
    assert_eq!(search_catalog(&db,serde_json::json!({"filter":{"offlineOnly":true}}),100).await.total,0);
    std::fs::rename(source,fixture.0.join("disconnected")).unwrap();assert!(crate::workflow::readable_path(&db,&id).await.is_err());close(db).await;
}

#[tokio::test]
async fn same_asset_has_independent_project_notes_and_nonmembers_are_rejected() {
    let fixture=Fixture::new();let source=fixture.0.join("NAS");std::fs::create_dir(&source).unwrap();std::fs::write(source.join("9.jpg"),b"image").unwrap();
    let db=fixture.database().await;let library_id=Uuid::new_v4().to_string();register_row(&db,&library_id,&source).await;
    let lib=db.register_library(&library_id,"NAS",&source).await.unwrap();crate::watcher::index_paths(&db,&library_id,&[source.to_string_lossy().into_owned()]).await.unwrap();
    let asset:String=sqlx::query_scalar("SELECT id FROM assets").fetch_one(&lib.pool).await.unwrap();
    for project in ["project-a","project-b","project-c"] {
        sqlx::query("INSERT INTO collections(id,name,created_at) VALUES(?,?,1)").bind(project).bind(project).execute(&db.master_pool).await.unwrap();
        sqlx::query("INSERT INTO collections(id,name,created_at) VALUES(?,?,1)").bind(project).bind(project).execute(&lib.pool).await.unwrap();
        if project!="project-c" {sqlx::query("INSERT INTO collection_assets(collection_id,asset_id) VALUES(?,?)").bind(project).bind(&asset).execute(&lib.pool).await.unwrap();}
    }
    for (project,note) in [("project-a","Warm oak for client A"),("project-b","White finish for client B")] {
        crate::commands::workflow::save_note(&db,project.into(),asset.clone(),note.into()).await.unwrap();
    }
    assert!(crate::commands::workflow::save_note(&db,"project-c".into(),asset.clone(),"invalid".into()).await.is_err());
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT collection_id,note FROM project_notes ORDER BY collection_id").fetch_all(&db.master_pool).await.unwrap();assert_eq!(rows.len(),2);assert_ne!(rows[0].1,rows[1].1);
    std::fs::rename(source,fixture.0.join("disconnected")).unwrap();close(db).await;
    let db=fixture.database().await;assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM project_notes").fetch_one(&db.master_pool).await.unwrap(),2);close(db).await;
}
impl Drop for Fixture {
    fn drop(&mut self) {
        #[cfg(windows)]
        for entry in walkdir::WalkDir::new(&self.0).follow_links(false).into_iter().filter_map(|entry|entry.ok()).filter(|entry|entry.file_type().is_file()) {
            if let Ok(metadata)=entry.metadata() {let mut permissions=metadata.permissions();permissions.set_readonly(false);let _=std::fs::set_permissions(entry.path(),permissions);}
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
async fn cached_preview_and_related_image_survive_disconnection_and_restart() {
    let fixture = Fixture::new();
    let source = fixture.0.join("NAS");
    std::fs::create_dir_all(source.join("kitchen")).unwrap();
    image::RgbImage::from_pixel(16, 8, image::Rgb([30, 150, 220])).save(source.join("kitchen/9.jpg")).unwrap();
    std::fs::write(source.join("kitchen/9.zip"), b"archive").unwrap();
    let db = fixture.database().await;
    let id = Uuid::new_v4().to_string();
    register_row(&db, &id, &source).await;
    let library = db.register_library(&id, "NAS", &source).await.unwrap();
    crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.unwrap();
    let asset: String = sqlx::query_scalar("SELECT id FROM assets WHERE extension='jpg'").fetch_one(&library.pool).await.unwrap();
    let thumb = library.catalog_dir.join("thumbs").join(&asset).join("preview.jpg");
    crate::preview::generate_image_thumb(&source.join("kitchen/9.jpg"), &thumb).unwrap();
    sqlx::query("UPDATE assets SET thumbnail_path=?,preview_status='done' WHERE id=?").bind(thumb.to_string_lossy().as_ref()).bind(&asset).execute(&library.pool).await.unwrap();
    sqlx::query("UPDATE preview_jobs SET status='done' WHERE asset_id=?").bind(&asset).execute(&library.pool).await.unwrap();
    close(db).await;
    std::fs::rename(&source, fixture.0.join("disconnected")).unwrap();
    let db = fixture.database().await;
    let library = db.get_library_handle(Some(&id)).await.unwrap();
    crate::commands::preview::enqueue_previews(&library.pool, &[asset], false).await.unwrap();
    let result = search_catalog(&db, serde_json::json!({"text":"kitchen"}), 100).await;
    assert_eq!(result.total, 2);
    for asset in result.assets {
        let cached = asset.thumbnail_path.or_else(|| asset.related_image.and_then(|image| image.thumbnail_path)).unwrap();
        assert_eq!(image::open(cached).unwrap().width(), 512);
    }
    let errors: i64 = sqlx::query_scalar("SELECT count(*) FROM preview_jobs WHERE status='error'").fetch_one(&library.pool).await.unwrap();
    assert_eq!(errors, 0);
    assert!(crate::watcher::index_paths(&db, &id, &[source.to_string_lossy().into_owned()]).await.is_err());
    assert_eq!(search_catalog(&db, serde_json::json!({}), 100).await.total, 2);
    close(db).await;
}

#[tokio::test]
async fn old_preview_errors_are_retried_once_after_generator_upgrade() {
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

    crate::preview::retry_failed_previews(&library.pool).await.unwrap();
    let states: Vec<(String, String)> = sqlx::query_as(
        "SELECT a.extension, p.status FROM assets a JOIN preview_jobs p ON p.asset_id = a.id ORDER BY a.extension"
    ).fetch_all(&library.pool).await.unwrap();
    assert_eq!(states, [("exr".into(), "queued".into()), ("hdr".into(), "queued".into()), ("max".into(), "queued".into())]);
    sqlx::query("UPDATE preview_jobs SET status = 'error' WHERE asset_id IN (SELECT id FROM assets WHERE extension = 'hdr')")
        .execute(&library.pool).await.unwrap();
    crate::preview::retry_failed_previews(&library.pool).await.unwrap();
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
