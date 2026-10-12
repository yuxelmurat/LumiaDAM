use anyhow::Result;
use sqlx::{Pool, Sqlite};
use std::collections::HashSet;
use crate::db::{LibraryHandle, resolve_path};
use crate::models::{FolderResult, SearchQuery};

/// Eski katalogdan klasörleri bir kez çıkarır; NAS'a erişmez.
pub async fn initialize(pool: &Pool<Sqlite>) -> Result<()> {
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='folders'")
        .fetch_one(pool).await?;
    if exists != 0 { return Ok(()); }
    let mut tx = pool.begin().await?;
    sqlx::query("CREATE TABLE folders(path TEXT PRIMARY KEY, name TEXT NOT NULL, parent TEXT NOT NULL); CREATE INDEX idx_folders_parent ON folders(parent);")
        .execute(&mut *tx).await?;
    let paths: Vec<String> = sqlx::query_scalar("SELECT file_path FROM assets").fetch_all(&mut *tx).await?;
    let mut folders = HashSet::new();
    for path in paths {
        let path = path.replace('\\', "/");
        let mut parent = path.as_str();
        while let Some((next, _)) = parent.rsplit_once('/') {
            if next.is_empty() { break; }
            folders.insert(next.to_string());
            parent = next;
        }
    }
    for path in folders {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", &path));
        sqlx::query("INSERT OR IGNORE INTO folders(path,name,parent) VALUES(?,?,?)")
            .bind(&path).bind(name).bind(parent).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Yalnız tamamlanmış tarama klasör listesini değiştirir; başarısız tarama eski kaydı korur.
pub async fn replace(pool: &Pool<Sqlite>, paths: &HashSet<String>) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM folders").execute(&mut *tx).await?;
    for path in paths {
        if path.is_empty() { continue; }
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        sqlx::query("INSERT INTO folders(path,name,parent) VALUES(?,?,?)")
            .bind(path).bind(name).bind(parent).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub fn includes_library(query: &SearchQuery, id: &str) -> bool {
    if let Some(ids) = &query.library_ids {
        if !ids.is_empty() { return ids.iter().any(|item| item == id); }
    }
    query.library_id.as_ref().filter(|id| !id.is_empty()).is_none_or(|selected| selected == id)
}

/// Klasör adı araması ve doğrudan alt klasör gezinmesi yalnız yerel SQLite üzerinden yapılır.
pub async fn search(libraries: &[LibraryHandle], query: &SearchQuery, page_size: i64, filters: &[String], filter_binds: &[String]) -> Result<(Vec<FolderResult>, i64)> {
    let text = query.text.trim();
    if text.is_empty() && query.folder_path.is_none() { return Ok((Vec::new(), 0)); }
    let offset = query.folder_page.unwrap_or(0).max(0).saturating_mul(page_size);
    let limit = offset.saturating_add(page_size);
    let mut folders = Vec::new();
    let mut total = 0;
    for lib in libraries.iter().filter(|lib| includes_library(query, &lib.id)) {
        let (mut conditions, mut binds) = crate::search_index::text_conditions(text, &lib.root_path.to_string_lossy(), false);
        if let Some(parent) = &query.folder_path {
            conditions.push("folders.parent = ?".into());
            binds.push(parent.clone());
        }
        if !filters.is_empty() {
            let filter = filters.join(" AND ");
            conditions.push(format!("CASE WHEN folders.path = '' THEN EXISTS (SELECT 1 FROM assets a WHERE {filter}) ELSE EXISTS (SELECT 1 FROM assets a WHERE replace(a.file_path,char(92),'/') >= folders.path || '/' AND replace(a.file_path,char(92),'/') < folders.path || '0' AND {filter}) END"));
            binds.extend(filter_binds.iter().cloned());
            binds.extend(filter_binds.iter().cloned());
        }
        let condition = if conditions.is_empty() { String::new() } else { format!("WHERE {}", conditions.join(" AND ")) };
        let source = "WITH candidates AS (SELECT rowid,path,name,parent FROM folders UNION ALL SELECT NULL,'',?,NULL)";
        let count_sql = format!("{source} SELECT COUNT(*) FROM candidates folders {condition}");
        let rows_sql = format!("{source} SELECT path,name FROM candidates folders {condition} ORDER BY path COLLATE NOCASE, path LIMIT ?");
        let root_name = lib.root_path.file_name().and_then(|name| name.to_str()).unwrap_or(&lib.name);
        let mut count = sqlx::query_scalar::<_, i64>(&count_sql).bind(root_name);
        let mut rows = sqlx::query_as::<_, (String, String)>(&rows_sql).bind(root_name);
        for value in binds { count = count.bind(value.clone()); rows = rows.bind(value); }
        total += count.fetch_one(&lib.pool).await?;
        let rows = rows.bind(limit).fetch_all(&lib.pool).await?;
        folders.extend(rows.into_iter().map(|(path, name)| FolderResult {
            full_path: resolve_path(&lib.root_path, &path), path, name, library_id: lib.id.clone(),
        }));
    }
    folders.sort_by(|a, b| a.path.to_ascii_lowercase().cmp(&b.path.to_ascii_lowercase())
        .then_with(|| a.path.cmp(&b.path)).then_with(|| a.library_id.cmp(&b.library_id)));
    Ok((folders.into_iter().skip(offset as usize).take(page_size as usize).collect(), total))
}

/// Tam klasör sınırıyla eşleşir; kitchen-old ve alt klasörlerdeki dosyalar karışmaz.
pub const DIRECT_FILES: &str = "replace(a.file_path, char(92), '/') >= ? AND replace(a.file_path, char(92), '/') < ? AND instr(substr(replace(a.file_path, char(92), '/'), length(?) + 1), '/') = 0";
