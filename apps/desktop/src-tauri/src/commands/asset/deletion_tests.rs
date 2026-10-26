use super::*;
use crate::db::{Database, LibraryHandle};
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    db: Database,
    library: LibraryHandle,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_source(None).await
    }

    async fn with_source(source: Option<PathBuf>) -> Self {
        let root = std::env::temp_dir().join(format!("lumiadam-delete-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = source.unwrap_or_else(|| root.join("source"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("asset.jpg"), b"korunacak kaynak").unwrap();
        let db = Database::new(root.join("master.sqlite").to_str().unwrap(), root.join("catalogs"))
            .await.unwrap();
        let library_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO libraries(id,name,root_paths,created_at) VALUES(?,'Test',?,1)")
            .bind(&library_id).bind(serde_json::to_string(&vec![&source]).unwrap())
            .execute(&db.master_pool).await.unwrap();
        let library = db.register_library(&library_id, "Test", &source).await.unwrap();
        sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES('asset','asset.jpg','asset.jpg','jpg','texture',1,1,1)")
            .execute(&library.pool).await.unwrap();
        sqlx::query("INSERT INTO tags(id,name) VALUES('tag','Test'); INSERT INTO asset_tags VALUES('asset','tag')")
            .execute(&library.pool).await.unwrap();
        Self { root, db, library }
    }

    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&self.library.pool).await.unwrap()
    }

    async fn close(self) {
        self.library.pool.close().await;
        self.db.master_pool.close().await;
        std::fs::remove_dir_all(self.root).unwrap();
    }
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "LUMIADAM_TEST_NAS_ROOT ile açıkça seçilmiş yazılabilir test paylaşımı gerekir"]
async fn nas_source_offline_reconnect_and_delete_protection() {
    let share = PathBuf::from(std::env::var("LUMIADAM_TEST_NAS_ROOT").unwrap());
    assert!(share.is_absolute() && share.is_dir());
    let source = share.join(format!("lumiadam-validation-{}", uuid::Uuid::new_v4()));
    let f = Fixture::with_source(Some(source.clone())).await;
    assert!(!f.library.catalog_dir.starts_with(&share));
    assert!(delete_catalog_asset("asset".into(), Some(true), &f.db).await.is_err());
    assert_eq!(std::fs::read(source.join("asset.jpg")).unwrap(), b"korunacak kaynak");
    assert_eq!(f.count("assets").await, 1);
    let offline = source.with_extension("offline");
    std::fs::rename(&source, &offline).unwrap();
    assert!(delete_catalog_asset("asset".into(), Some(true), &f.db).await.is_err());
    assert_eq!(f.count("assets").await, 1);
    std::fs::rename(&offline, &source).unwrap();
    assert_eq!(std::fs::read(source.join("asset.jpg")).unwrap(), b"korunacak kaynak");
    assert_eq!(std::fs::read_dir(&source).unwrap().count(), 1);
    delete_catalog_asset("asset".into(), Some(false), &f.db).await.unwrap();
    assert_eq!(f.count("assets").await, 0);
    // Yalnız testin oluşturduğu dosya ve boş test klasörü kaldırılır.
    std::fs::remove_file(source.join("asset.jpg")).unwrap();
    std::fs::remove_dir(source).unwrap();
    f.close().await;
}

#[tokio::test]
async fn unavailable_source_preserves_catalog() {
    let f = Fixture::new().await;
    std::fs::rename(&f.library.root_path, f.root.join("disconnected-source")).unwrap();
    let result = delete_catalog_asset("asset".into(), Some(true), &f.db).await;
    assert!(result.is_err(), "Çevrimdışı kaynak başarılı silme gibi raporlanmamalı");
    assert_eq!(f.count("assets").await, 1);
    assert_eq!(f.count("asset_tags").await, 1);
    f.close().await;
}

#[cfg(windows)]
#[tokio::test]
async fn recycle_failure_preserves_file_and_catalog() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new().await;
    let path = f.library.root_path.join("asset.jpg");
    let locked = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
    let result = delete_catalog_asset("asset".into(), Some(true), &f.db).await;
    assert!(result.is_err(), "Geri dönüşüm kutusu hatası kullanıcıya ulaşmalı");
    drop(locked);
    assert_eq!(std::fs::read(&path).unwrap(), b"korunacak kaynak");
    assert_eq!(f.count("assets").await, 1);
    assert_eq!(f.count("asset_tags").await, 1);
    f.close().await;
}

#[tokio::test]
async fn catalog_failure_rolls_back_related_records() {
    let f = Fixture::new().await;
    sqlx::query("CREATE TRIGGER refuse_delete BEFORE DELETE ON assets BEGIN SELECT RAISE(ABORT, 'test delete failure'); END")
        .execute(&f.library.pool).await.unwrap();
    let result = delete_catalog_asset("asset".into(), Some(false), &f.db).await;
    assert!(result.is_err(), "Katalog yazma hatası yutulmamalı");
    assert_eq!(f.count("assets").await, 1);
    assert_eq!(f.count("asset_tags").await, 1);
    assert!(f.library.root_path.join("asset.jpg").exists());
    f.close().await;
}

#[tokio::test]
async fn outside_library_path_is_never_deleted() {
    let f = Fixture::new().await;
    let outside = f.root.join("outside.jpg");
    std::fs::write(&outside, b"kapsam disi").unwrap();
    sqlx::query("UPDATE assets SET file_path = '../outside.jpg' WHERE id = 'asset'")
        .execute(&f.library.pool).await.unwrap();
    let result = delete_catalog_asset("asset".into(), Some(true), &f.db).await;
    assert!(result.is_err(), "Kütüphane dışına çıkan yol reddedilmeli");
    assert_eq!(std::fs::read(outside).unwrap(), b"kapsam disi");
    assert_eq!(f.count("assets").await, 1);
    f.close().await;
}

#[tokio::test]
async fn catalog_only_removal_keeps_source_file() {
    let f = Fixture::new().await;
    delete_catalog_asset("asset".into(), Some(false), &f.db).await.unwrap();
    assert!(f.library.root_path.join("asset.jpg").exists());
    assert_eq!(f.count("assets").await, 0);
    assert_eq!(f.count("asset_tags").await, 0);
    f.close().await;
}
