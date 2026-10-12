use crate::{db::connect_library_sqlite, models::SearchPayload};
use sqlx::SqlitePool;

struct Catalog(std::path::PathBuf);
impl Catalog {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lumiadam-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    async fn open(&self) -> SqlitePool { connect_library_sqlite(&self.0.join("library.sqlite")).await.unwrap() }
}
impl Drop for Catalog { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

async fn insert(pool: &SqlitePool, id: &str, path: &str) {
    sqlx::query("INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) VALUES(?,?,?,'max','3d_model',1,1,1)")
        .bind(id).bind(path).bind(path.rsplit('/').next().unwrap()).execute(pool).await.unwrap();
}

#[tokio::test]
async fn path_index_is_created_backfilled_and_kept_consistent() {
    let fixture = Catalog::new();
    let pool = fixture.open().await;
    insert(&pool, "one", "room/kitchen/one.max").await;
    let found: i64 = sqlx::query_scalar("SELECT count(*) FROM assets_path_fts WHERE assets_path_fts MATCH '\"kitch\"'").fetch_one(&pool).await.unwrap();
    assert_eq!(found, 1);
    sqlx::query("UPDATE assets SET file_path='room/bedroom/one.max' WHERE id='one'").execute(&pool).await.unwrap();
    let found: i64 = sqlx::query_scalar("SELECT count(*) FROM assets_path_fts WHERE assets_path_fts MATCH '\"kitch\"'").fetch_one(&pool).await.unwrap();
    assert_eq!(found, 0);
    sqlx::query("DELETE FROM assets WHERE id='one'").execute(&pool).await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn preview_update_does_not_rewrite_text_fts() {
    let fixture = Catalog::new();
    let pool = fixture.open().await;
    insert(&pool, "one", "room/one.max").await;
    let mut connection = pool.acquire().await.unwrap();
    let before: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&mut *connection).await.unwrap();
    sqlx::query("UPDATE assets SET preview_status='done' WHERE id='one'").execute(&mut *connection).await.unwrap();
    let after: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&mut *connection).await.unwrap();
    assert!(after - before <= 3, "Preview changed {} rows; FTS was rewritten", after - before);
    drop(connection);
    pool.close().await;
}

#[test]
fn legacy_payload_still_deserializes() {
    let _: SearchPayload = serde_json::from_value(serde_json::json!({"query":{"text":"","filter":{},"sort":{"field":"name","order":"asc"}},"page":0,"pageSize":50})).unwrap();
}

async fn database(fixture: &Catalog, pool: &SqlitePool) -> crate::db::Database {
    let library = crate::db::LibraryHandle {
        id: "test-library".into(), name: "Archive".into(), root_path: std::path::PathBuf::from("C:/Asset Archive"), catalog_dir: fixture.0.clone(),
        scan_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())), scan_processed: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)), scan_full: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)), pool: pool.clone(),
    };
    crate::db::Database {
        master_pool: pool.clone(), pool: pool.clone(), libraries: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::from([(library.id.clone(), library)]))),
        storage_root: fixture.0.clone(), default_storage_root: fixture.0.clone(), storage_errors: vec![], preview_wakeup: std::sync::Arc::new(tokio::sync::Notify::new()),
    }
}

fn payload(text: &str, scope: Option<&str>, filter: serde_json::Value) -> SearchPayload {
    serde_json::from_value(serde_json::json!({"query":{"text":text,"filter":filter,"sort":{"field":"name","order":"asc"}},"page":0,"pageSize":100,"resultScope":scope})).unwrap()
}

async fn legacy_ids(pool: &SqlitePool, text: &str) -> Vec<String> {
    let mut conditions = Vec::new();
    let mut binds = Vec::new();
    for word in text.split_whitespace() {
        conditions.push("(a.id IN (SELECT id FROM assets_fts WHERE assets_fts MATCH ?) OR instr(lower(replace('C:/Asset Archive/' || a.file_path,char(92),'/')),lower(?))>0)");
        binds.push(format!("\"{}\"*", word.replace('"', "\"\"")));
        binds.push(word.replace('\\', "/"));
    }
    let sql = format!("SELECT id FROM assets a WHERE {} ORDER BY id", conditions.join(" AND "));
    let mut query = sqlx::query_scalar::<_, String>(&sql);
    for value in binds { query = query.bind(value); }
    query.fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn optimized_search_equals_legacy_for_paths_roots_short_unicode_and_literals() {
    let fixture = Catalog::new();
    let pool = fixture.open().await;
    for (i, path) in ["rooms/kitchen/one.max", "rooms/bedroom/two.max", "100%_done/[test]/three.max", "ÜST/İstanbul/été/four.max", "slash\\deep\\five.max", "quote\"name/six.max"].iter().enumerate() {
        insert(&pool, &i.to_string(), path).await;
    }
    sqlx::query("UPDATE assets SET description='wooden upholstered' WHERE id='1'").execute(&pool).await.unwrap();
    let db = database(&fixture, &pool).await;
    for text in ["kitch", "rooms", "o", "oo", "%", "_", "100%_", "[test]", "ÜST", "üst", "İstanbul", "istanbul", "été", "slash\\deep", "quote\"name", "Archive", "Archive/rooms", "ve/rooms", "C:/Asset", "rooms uphol", "kitch wood", "bedroom wood"] {
        let expected = legacy_ids(&pool, text).await;
        let result = crate::commands::asset::search_catalogs(payload(text, Some("files"), serde_json::json!({})), &db).await.unwrap();
        let mut found: Vec<_> = result.assets.into_iter().map(|asset| asset.id).collect(); found.sort();
        assert_eq!(found, expected, "Search mismatch: {text}");
    }
    pool.close().await;
}

#[tokio::test]
async fn scopes_and_cache_follow_asset_folder_tag_collection_mutations() {
    let fixture = Catalog::new(); let pool = fixture.open().await;
    insert(&pool, "one", "kitchen/one.max").await;
    let paths = std::collections::HashSet::from(["kitchen".to_string(), "empty-kitchen".to_string()]);
    crate::folders::replace(&pool, &paths).await.unwrap();
    let db = database(&fixture, &pool).await;
    let search = |scope| payload("kitch", scope, serde_json::json!({"rating":{"min":3,"max":5}}));
    let before = crate::commands::asset::search_catalogs(search(None), &db).await.unwrap();
    assert_eq!((before.total, before.folder_total), (0, 0));
    sqlx::query("UPDATE assets SET rating=4 WHERE id='one'").execute(&pool).await.unwrap();
    let after = crate::commands::asset::search_catalogs(search(None), &db).await.unwrap();
    assert_eq!((after.total, after.folder_total), (1, 1));
    let files = crate::commands::asset::search_catalogs(search(Some("files")), &db).await.unwrap();
    let folders = crate::commands::asset::search_catalogs(search(Some("folders")), &db).await.unwrap();
    assert_eq!((files.total, files.folder_total, folders.total, folders.folder_total), (1, 0, 0, 1));
    let revision = || sqlx::query_scalar::<_, i64>("SELECT revision FROM search_revision");
    for mutation in [
        "INSERT INTO tags(id,name) VALUES('tag','wood')", "INSERT INTO asset_tags VALUES('one','tag')", "DELETE FROM asset_tags",
        "INSERT INTO collections(id,name,created_at) VALUES('c','Collection',1)", "INSERT INTO collection_assets VALUES('c','one')",
        "UPDATE collections SET name='Renamed'", "DELETE FROM collection_assets", "DELETE FROM collections", "DELETE FROM tags",
        "UPDATE folders SET name='Kitchen' WHERE path='kitchen'", "UPDATE assets SET preview_status='done' WHERE id='one'",
    ] {
        let before = revision().fetch_one(&pool).await.unwrap();
        sqlx::query(mutation).execute(&pool).await.unwrap();
        assert!(revision().fetch_one(&pool).await.unwrap() > before, "Revision missing: {mutation}");
    }
    let refreshed = crate::commands::asset::search_catalogs(search(Some("files")), &db).await.unwrap();
    assert_eq!(refreshed.assets[0].preview_status, "done");
    sqlx::query("DROP TABLE folders").execute(&pool).await.unwrap();
    // Dosya sorgusu klasor sorgusunun hatasindan bagimsizdir.
    let result = crate::commands::asset::search_catalogs(payload("one", Some("files"), serde_json::json!({})), &db).await.unwrap();
    assert_eq!(result.total, 1);
    pool.close().await;
}

#[tokio::test]
async fn migration_backfills_old_catalog_and_is_idempotent() {
    let fixture = Catalog::new();
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(fixture.0.join("library.sqlite")).create_if_missing(true)).await.unwrap();
    sqlx::query(include_str!("../../migrations/002_library_schema.sql")).execute(&pool).await.unwrap();
    insert(&pool, "old", "old/kitchen/old.max").await;
    crate::folders::initialize(&pool).await.unwrap();
    super::initialize(&pool).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM assets_path_fts WHERE assets_path_fts MATCH '\"kitch\"'").fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
    let before: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&pool).await.unwrap();
    super::initialize(&pool).await.unwrap();
    let after: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&pool).await.unwrap();
    assert_eq!(before, after);
    pool.close().await;
}

#[tokio::test]
async fn journal_and_pragmas_apply_to_every_connection() {
    let fixture = Catalog::new(); let pool = fixture.open().await;
    let mut connections = Vec::new();
    for _ in 0..5 { connections.push(pool.acquire().await.unwrap()); }
    for connection in &mut connections {
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode").fetch_one(&mut **connection).await.unwrap();
        #[cfg(windows)] assert_eq!(mode, "wal");
        #[cfg(not(windows))] assert_eq!(mode, "delete");
        let cache: i64 = sqlx::query_scalar("PRAGMA cache_size").fetch_one(&mut **connection).await.unwrap();
        let temp: i64 = sqlx::query_scalar("PRAGMA temp_store").fetch_one(&mut **connection).await.unwrap();
        assert_eq!((cache,temp), (-4096,2));
    }
    drop(connections); pool.close().await;
}

/// Yerel gecici katalog; canli NAS veya kullanici kataloglari okunmaz.
#[tokio::test]
#[ignore = "Reproducible synthetic benchmark; run explicitly with --ignored --nocapture"]
async fn synthetic_search_benchmark() {
    let fixture = Catalog::new(); let pool = fixture.open().await;
    let started = std::time::Instant::now();
    sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<50000) INSERT INTO assets(id,file_path,file_name,extension,kind,size_bytes,modified_at,indexed_at) SELECT CAST(x AS TEXT), printf('archive/%s/group-%04d/%08d.max',CASE WHEN x%500=0 THEN 'kitchen' ELSE 'bedroom' END,x%1000,x),printf('%08d.max',x),'max','3d_model',x,x,x FROM n")
        .execute(&pool).await.unwrap();
    let generated_ms = started.elapsed().as_millis();
    let db = database(&fixture, &pool).await;
    let before_sql = "SELECT count(*) FROM assets a WHERE (a.id IN (SELECT id FROM assets_fts WHERE assets_fts MATCH ?) OR instr(lower(replace('C:/Asset Archive/' || a.file_path,char(92),'/')),lower(?))>0)";
    let (conditions, binds) = super::text_conditions("kitch", "C:/Asset Archive", true);
    let after_sql = format!("SELECT count(*) FROM assets a WHERE {}", conditions.join(" AND "));
    let mut before_times = Vec::new(); let mut after_times = Vec::new();
    for _ in 0..12 {
        let start = std::time::Instant::now();
        let before: i64 = sqlx::query_scalar(before_sql).bind("\"kitch\"*").bind("kitch").fetch_one(&pool).await.unwrap();
        before_times.push(start.elapsed().as_secs_f64()*1000.0);
        let start = std::time::Instant::now();
        let mut query = sqlx::query_scalar::<_, i64>(&after_sql);
        for bind in &binds { query=query.bind(bind); }
        let after = query.fetch_one(&pool).await.unwrap();
        after_times.push(start.elapsed().as_secs_f64()*1000.0);
        assert_eq!(before,100); assert_eq!(after,before);
    }
    before_times.sort_by(f64::total_cmp); after_times.sort_by(f64::total_cmp);
    let request = || payload("kitch", Some("files"), serde_json::json!({}));
    let start=std::time::Instant::now();
    let first=crate::commands::asset::search_catalogs(request(), &db).await.unwrap();
    let first_ms=start.elapsed().as_secs_f64()*1000.0;
    let start=std::time::Instant::now();
    let cached=crate::commands::asset::search_catalogs(request(), &db).await.unwrap();
    let cached_ms=start.elapsed().as_secs_f64()*1000.0;
    assert_eq!((first.total,cached.total),(100,100));
    let plan_sql = format!("EXPLAIN QUERY PLAN {after_sql}");
    let mut plan=sqlx::query_as::<_,(i64,i64,i64,String)>(&plan_sql);
    for bind in &binds { plan=plan.bind(bind); }
    let plan=plan.fetch_all(&pool).await.unwrap();
    println!("SEARCH_BENCH assets=50000 matches=100 iterations=12 generated_ms={generated_ms} legacy_median_ms={:.3} indexed_median_ms={:.3} first_page_ms={first_ms:.3} cached_page_ms={cached_ms:.3}",before_times[6],after_times[6]);
    for (_,_,_,detail) in plan { println!("SEARCH_PLAN {detail}"); }
    let direct_plan: Vec<(i64,i64,i64,String)> = sqlx::query_as("EXPLAIN QUERY PLAN SELECT id FROM assets a WHERE replace(a.file_path,char(92),'/')>='archive/kitchen/' AND replace(a.file_path,char(92),'/')<'archive/kitchen0'").fetch_all(&pool).await.unwrap();
    for (_,_,_,detail) in direct_plan { println!("DIRECT_PLAN {detail}"); }
    pool.close().await;
}

#[tokio::test]
async fn transferred_catalog_preserves_trigram_asset_and_folder_results() {
    let fixture = Catalog::new(); let pool = fixture.open().await;
    insert(&pool,"deleted","other/deleted.max").await;
    insert(&pool,"keep","kitchen/keep.max").await;
    sqlx::query("DELETE FROM assets WHERE id='deleted'").execute(&pool).await.unwrap();
    crate::folders::replace(&pool, &std::collections::HashSet::from(["kitchen".to_string()])).await.unwrap();
    let destination = crate::storage::prepare_catalog(&fixture.0.join("destination"), "Transferred", &uuid::Uuid::new_v4().to_string(), Some(&fixture.0)).await.unwrap();
    let copy = connect_library_sqlite(&destination.join("library.sqlite")).await.unwrap();
    let db = database(&fixture, &copy).await;
    let result = crate::commands::asset::search_catalogs(payload("kitch", None, serde_json::json!({})), &db).await.unwrap();
    assert_eq!((result.total,result.folder_total),(1,1));
    assert_eq!(result.assets[0].id,"keep");
    let source_count: i64=sqlx::query_scalar("SELECT count(*) FROM assets").fetch_one(&pool).await.unwrap();
    assert_eq!(source_count,1);
    copy.close().await; pool.close().await;
}

#[tokio::test]
async fn cached_filters_change_after_writes_from_another_connection() {
    let fixture=Catalog::new(); let pool=fixture.open().await;
    insert(&pool,"one","kitchen/one.max").await;
    crate::folders::replace(&pool,&std::collections::HashSet::from(["kitchen".into()])).await.unwrap();
    let db=database(&fixture,&pool).await;
    let mut writer=pool.acquire().await.unwrap();
    sqlx::query("INSERT INTO tags(id,name) VALUES('t','Wood'); INSERT INTO collections(id,name,created_at) VALUES('c','Furniture',1)").execute(&mut *writer).await.unwrap();
    for (filter, insert_sql, delete_sql) in [
        (serde_json::json!({"tags":["t"]}), "INSERT INTO asset_tags VALUES('one','t')", "DELETE FROM asset_tags"),
        (serde_json::json!({"collectionIds":["c"]}), "INSERT INTO collection_assets VALUES('c','one')", "DELETE FROM collection_assets"),
    ] {
        let request=|| payload("kitch",None,filter.clone());
        let before=crate::commands::asset::search_catalogs(request(),&db).await.unwrap();
        assert_eq!((before.total,before.folder_total),(0,0));
        sqlx::query(insert_sql).execute(&mut *writer).await.unwrap();
        let after=crate::commands::asset::search_catalogs(request(),&db).await.unwrap();
        assert_eq!((after.total,after.folder_total),(1,1));
        sqlx::query(delete_sql).execute(&mut *writer).await.unwrap();
        let removed=crate::commands::asset::search_catalogs(request(),&db).await.unwrap();
        assert_eq!((removed.total,removed.folder_total),(0,0));
    }
    drop(writer); pool.close().await;
}
