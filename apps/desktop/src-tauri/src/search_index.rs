use crate::db::LibraryHandle;
use crate::models::SearchResult;
use sqlx::SqlitePool;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// Geçiş tek transaction içinde tamamlanır; mevcut katalog yalnız bir kez taranır.
pub async fn initialize(pool: &SqlitePool) -> anyhow::Result<()> {
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='search_revision'").fetch_one(pool).await?;
    if exists != 0 { return Ok(()); }
    let mut tx = pool.begin().await?;
    sqlx::query(include_str!("../migrations/003_search_index.sql")).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// VACUUM aktariminda degisen rowid baglantilarini ayni transaction icinde yeniler.
pub async fn rebuild_after_snapshot(connection: &mut sqlx::SqliteConnection) -> anyhow::Result<()> {
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='assets_path_fts'").fetch_one(&mut *connection).await?;
    if exists == 0 { return Ok(()); }
    sqlx::query("DELETE FROM assets_path_fts; INSERT INTO assets_path_fts(rowid,path) SELECT rowid,lower(replace(file_path,char(92),'/')) FROM assets; DELETE FROM folders_path_fts; INSERT INTO folders_path_fts(rowid,path) SELECT rowid,lower(replace(path,char(92),'/')) FROM folders;")
        .execute(&mut *connection).await?;
    Ok(())
}

/// SQLite lower yalnız ASCII harflerini küçültür; Unicode davranışını değiştirmeyiz.
pub fn text_conditions(text: &str, root: &str, assets: bool) -> (Vec<String>, Vec<String>) {
    let root = format!("{}/", root.replace('\\', "/")).to_ascii_lowercase();
    let mut conditions = Vec::new();
    let mut binds = Vec::new();
    for word in text.split_whitespace() {
        let normalized = word.replace('\\', "/").to_ascii_lowercase();
        if root.contains(&normalized) { continue; }
        let crosses_root = normalized.char_indices().skip(1).any(|(i, _)| root.ends_with(&normalized[..i]));
        let indexed = normalized.chars().count() >= 3 && !crosses_root && !normalized.contains('\0');
        let table = if assets { "assets_path_fts" } else { "folders_path_fts" };
        let path = if assets { "a.file_path" } else { "folders.path" };
        let rowid = if assets { "a.rowid" } else { "folders.rowid" };
        let exact = format!("instr(lower(replace(? || {path}, char(92), '/')), ?) > 0");
        if indexed {
            // UNION seçici adayları birleştirir; tüm assets tablosundaki OR taramasını önler.
            let path_candidates = format!("SELECT rowid FROM {table} WHERE {table} MATCH ? AND instr(path, ?) > 0");
            if assets {
                conditions.push(format!("{rowid} IN (SELECT rowid FROM assets_fts WHERE assets_fts MATCH ? UNION {path_candidates})"));
                binds.push(format!("\"{}\"*", word.replace('"', "\"\"")));
            } else {
                conditions.push(format!("{rowid} IN ({path_candidates})"));
            }
            binds.push(format!("\"{}\"", normalized.replace('"', "\"\"")));
            binds.push(normalized);
        } else {
            if assets {
                conditions.push(format!("(a.rowid IN (SELECT rowid FROM assets_fts WHERE assets_fts MATCH ?) OR {exact})"));
                binds.push(format!("\"{}\"*", word.replace('"', "\"\"")));
            } else { conditions.push(exact); }
            binds.push(root.clone());
            binds.push(normalized);
        }
    }
    (conditions, binds)
}

pub async fn revisions(libraries: &[LibraryHandle]) -> anyhow::Result<String> {
    let mut versions = Vec::new();
    for library in libraries {
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM search_revision WHERE id=1").fetch_one(&library.pool).await?;
        versions.push(format!("{:?}:{:?}:{:?}:{revision}", library.id, library.catalog_dir, library.root_path));
    }
    versions.sort();
    Ok(versions.join("|"))
}

struct Cached { key: String, result: SearchResult, bytes: usize }
static CACHE: OnceLock<Mutex<VecDeque<Cached>>> = OnceLock::new();
const CACHE_BYTES: usize = 8 * 1024 * 1024;
pub fn cached(key: &str) -> Option<SearchResult> {
    let mut cache = CACHE.get_or_init(|| Mutex::new(VecDeque::new())).lock().ok()?;
    let index = cache.iter().position(|item| item.key == key)?;
    let item = cache.remove(index)?;
    let result = item.result.clone();
    cache.push_back(item);
    Some(result)
}
pub fn cache(key: String, result: &SearchResult) {
    let Ok(bytes) = serde_json::to_vec(result).map(|value| value.len() + key.len()) else { return };
    if bytes > CACHE_BYTES { return; }
    let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(VecDeque::new())).lock() else { return };
    cache.retain(|item| item.key != key);
    while cache.len() >= 32 || cache.iter().map(|item| item.bytes).sum::<usize>() + bytes > CACHE_BYTES { cache.pop_front(); }
    cache.push_back(Cached { key, result: result.clone(), bytes });
}

#[cfg(test)]
mod tests;
