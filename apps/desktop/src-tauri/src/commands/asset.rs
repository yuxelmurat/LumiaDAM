use std::sync::Arc;
use std::path::Path;
use std::collections::{HashMap, HashSet};
use sqlx::{QueryBuilder, Sqlite};
use tauri::{State, Emitter};
use serde_json;

use crate::AppState;
use crate::models::{Asset, RelatedImage, SearchPayload, SearchResult, AssetPatch};
use crate::db::{resolve_path, make_relative_path, is_legacy_thumbnail};

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, sqlx::FromRow)]
struct RawAssetRow {
    pub id: String,
    pub file_path: String,
    pub file_name: String,
    pub extension: String,
    pub kind: String,
    pub size_bytes: i64,
    pub modified_at: i64,
    pub indexed_at: i64,
    pub rating: i64,
    pub color_label: String,
    pub is_favorite: i64,
    pub description: String,
    pub thumbnail_path: Option<String>,
    pub preview_status: String,
    pub metadata: String,
    pub tags: Option<String>,
    pub collections: Option<String>,
}

/// Build an Asset from a raw SQLite row, resolving relative paths to absolute paths
fn row_to_asset(root: Option<&Path>, row: RawAssetRow) -> Asset {
    let mut metadata: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_str(&row.metadata).unwrap_or_default();
    if let (Some(r), Some(pm)) = (root, metadata.get("preview_model").and_then(|v| v.as_str())) {
        let abs_pm = resolve_path(r, pm);
        metadata.insert("preview_model".to_string(), serde_json::Value::String(abs_pm));
    }
    let tags: Vec<String> = row.tags
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let collections: Vec<String> = row.collections
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();

    let resolved_file_path = match root {
        Some(r) => resolve_path(r, &row.file_path),
        None => row.file_path,
    };
    let resolved_thumb_path = match (root, row.thumbnail_path.filter(|path| !is_legacy_thumbnail(path))) {
        (Some(r), Some(t)) => Some(resolve_path(r, &t)),
        (_, t) => t,
    };

    Asset {
        library_id: None,
        path_labels: Default::default(),
        inferred_labels: Default::default(),
        label_overrides: Default::default(),
        related_mode: "auto".into(),
        group_files: Vec::new(),
        offline_path: None,
        offline_stale: false,
        id: row.id,
        file_path: resolved_file_path,
        file_name: row.file_name,
        extension: row.extension,
        kind: row.kind,
        size_bytes: row.size_bytes,
        modified_at: row.modified_at,
        indexed_at: row.indexed_at,
        rating: row.rating,
        color_label: row.color_label,
        is_favorite: row.is_favorite != 0,
        description: row.description,
        tags,
        collections,
        thumbnail_path: resolved_thumb_path,
        related_image: None,
        preview_status: row.preview_status,
        metadata,
    }
}

// Yalnız katalogdaki aynı klasör ve aynı gövde adlı görseller ilişkilendirilir.
// Filtreler görseli gizlese de ilişki bulunur; kaynak sürücü tekrar taranmaz.
async fn attach_related_images(pool: &sqlx::SqlitePool, root: &Path, assets: &mut [Asset]) -> Result<()> {
    const SUFFIXES: [&str; 5] = ["", "_preview", "-preview", "_render", "-render"];
    const EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];
    let mut candidates = Vec::with_capacity(assets.len());
    let mut unique = HashSet::new();

    for asset in assets.iter() {
        let mut paths = Vec::new();
        if matches!(asset.kind.as_str(), "3d_model" | "material" | "other") {
            let relative = make_relative_path(root, Path::new(&asset.file_path));
            let (directory, name) = relative.rsplit_once('/').unwrap_or(("", &relative));
            if let Some((stem, _)) = name.rsplit_once('.') {
                for suffix in SUFFIXES {
                    for ext in EXTENSIONS {
                        let file = format!("{stem}{suffix}.{ext}");
                        let path = if directory.is_empty() { file } else { format!("{directory}/{file}") };
                        unique.insert(path.to_ascii_lowercase());
                        paths.push(path.to_ascii_lowercase());
                    }
                }
            }
        }
        candidates.push(paths);
    }
    if unique.is_empty() { return Ok(()); }

    let mut images = HashMap::new();
    let paths: Vec<_> = unique.into_iter().collect();
    for chunk in paths.chunks(400) {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT id, file_path, file_name, thumbnail_path FROM assets WHERE file_path COLLATE NOCASE IN (");
        let mut separated = query.separated(", ");
        for path in chunk { separated.push_bind(path); }
        separated.push_unseparated(") AND lower(extension) IN ('jpg','jpeg','png','webp')");
        let rows: Vec<(String, String, String, Option<String>)> = query.build_query_as().fetch_all(pool).await.map_err(|e| e.to_string())?;
        for (id, path, name, thumbnail) in rows { images.insert(path.to_ascii_lowercase(), (id, path, name, thumbnail)); }
    }

    for (asset, paths) in assets.iter_mut().zip(candidates) {
        if let Some((id, path, name, thumbnail)) = paths.iter().find_map(|path| images.get(path)) {
            let thumbnail_path = thumbnail.as_ref()
                .filter(|thumb| !is_legacy_thumbnail(thumb))
                .map(|thumb| resolve_path(root, thumb));
            asset.related_image = Some(RelatedImage {
                id: id.clone(),
                file_path: resolve_path(root, path),
                file_name: name.clone(),
                thumbnail_path,
            });
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn search_assets(
    payload: SearchPayload,
    state: State<'_, Arc<AppState>>,
) -> Result<SearchResult> {
    search_catalogs(payload, &state.db).await
}

pub(crate) async fn search_catalogs(payload: SearchPayload, db: &crate::db::Database) -> Result<SearchResult> {
    let started = std::time::Instant::now();
    if !db.storage_errors.is_empty() {
        return Err("Some catalogs could not be opened. Check the transfer errors in Storage settings.".into());
    }
    if !matches!(payload.result_scope.as_deref(), None | Some("files") | Some("folders")) {
        return Err("Invalid result scope".into());
    }
    let mut libraries = db.get_all_libraries().await;
    libraries.retain(|lib| crate::folders::includes_library(&payload.query, &lib.id));
    libraries.sort_by(|a, b| a.id.cmp(&b.id));
    let before = crate::search_index::revisions(&libraries).await.map_err(|e| e.to_string())?;
    let key = format!("{:?}|{before}", payload);
    if let Some(result) = crate::search_index::cached(&key) {
        tracing::debug!(elapsed_ms = started.elapsed().as_millis() as u64, cache_hit = true, "Catalog search");
        return Ok(result);
    }
    let result = search_uncached(&payload, &libraries).await?;
    if before == crate::search_index::revisions(&libraries).await.map_err(|e| e.to_string())? {
        crate::search_index::cache(key, &result);
    }
    tracing::debug!(elapsed_ms = started.elapsed().as_millis() as u64, cache_hit = false, scope = ?payload.result_scope, "Catalog search");
    Ok(result)
}

async fn search_uncached(payload: &SearchPayload, all_libs: &[crate::db::LibraryHandle]) -> Result<SearchResult> {
    let limit = payload.page_size.clamp(1, 500);
    let offset = payload.page.max(0).saturating_mul(limit);
    let f = &payload.query.filter;

    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if let Some(labels) = &f.path_labels {
        for (column, value) in [("year", &labels.year), ("source", &labels.source), ("room", &labels.room), ("renderer", &labels.renderer)] {
            if !value.is_empty() {
                conditions.push(format!("a.id IN (SELECT asset_id FROM asset_workflow WHERE {column} = ?)"));
                binds.push(value.clone());
            }
        }
    }
    if f.offline_only == Some(true) {
        conditions.push("a.id IN (SELECT o.asset_id FROM offline_assets o WHERE o.size_bytes=a.size_bytes AND o.modified_at=a.modified_at)".into());
    }

    if let Some(kinds) = &f.kinds {
        if !kinds.is_empty() {
            let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            conditions.push(format!("a.kind IN ({})", placeholders));
            binds.extend(kinds.clone());
        }
    }

    if let Some(color_labels) = &f.color_labels {
        if !color_labels.is_empty() {
            let placeholders = color_labels.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            conditions.push(format!("a.color_label IN ({})", placeholders));
            binds.extend(color_labels.clone());
        }
    }

    if let Some(rating) = &f.rating {
        conditions.push("a.rating >= ? AND a.rating <= ?".into());
        binds.push(rating.min.to_string());
        binds.push(rating.max.to_string());
    }

    if let Some(is_fav) = f.is_favorite {
        if is_fav {
            conditions.push("a.is_favorite = 1".into());
        } else {
            conditions.push("a.is_favorite = 0".into());
        }
    }

    if let Some(dr) = &f.date_range {
        if let Some(from) = dr.from {
            conditions.push("a.modified_at >= ?".into());
            binds.push(from.to_string());
        }
        if let Some(to) = dr.to {
            conditions.push("a.modified_at <= ?".into());
            binds.push(to.to_string());
        }
    }

    if let Some(sr) = &f.size_range {
        if let Some(min) = sr.min {
            conditions.push("a.size_bytes >= ?".into());
            binds.push(min.to_string());
        }
        if let Some(max) = sr.max {
            conditions.push("a.size_bytes <= ?".into());
            binds.push(max.to_string());
        }
    }

    if let Some(ps) = &f.preview_status {
        if !ps.is_empty() {
            if ps == "missing" || ps == "needs_preview" {
                conditions.push("a.preview_status IN ('none', 'error')".into());
            } else if ps == "has_preview" {
                conditions.push("a.preview_status = 'done'".into());
            } else {
                conditions.push("a.preview_status = ?".into());
                binds.push(ps.clone());
            }
        }
    }

    if let Some(exts) = &f.extensions {
        if !exts.is_empty() {
            let placeholders = exts.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            conditions.push(format!("a.extension IN ({})", placeholders));
            binds.extend(exts.clone());
        }
    }

    // Tag filter: supports logic "and" (has all tags), "or" (has any tag), "and_or" (has primary tag AND any of remaining), "none" (untagged)
    let tag_logic = f.tag_logic.as_deref().unwrap_or("or");
    if tag_logic == "none" {
        conditions.push("a.id NOT IN (SELECT asset_id FROM asset_tags)".into());
    } else if let Some(tags) = &f.tags {
        if !tags.is_empty() {
            if tag_logic == "and" {
                let placeholders = tags.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                conditions.push(format!(
                    "a.id IN (SELECT asset_id FROM asset_tags WHERE tag_id IN ({}) GROUP BY asset_id HAVING COUNT(DISTINCT tag_id) = ?)",
                    placeholders
                ));
                binds.extend(tags.clone());
                binds.push(tags.len().to_string());
            } else if tag_logic == "and_or" {
                if tags.len() <= 1 {
                    conditions.push("a.id IN (SELECT asset_id FROM asset_tags WHERE tag_id = ?)".into());
                    binds.push(tags[0].clone());
                } else {
                    let primary_tag = &tags[0];
                    let remaining_tags = &tags[1..];
                    let placeholders = remaining_tags.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    conditions.push(format!(
                        "a.id IN (SELECT asset_id FROM asset_tags WHERE tag_id = ?) AND a.id IN (SELECT asset_id FROM asset_tags WHERE tag_id IN ({}))",
                        placeholders
                    ));
                    binds.push(primary_tag.clone());
                    binds.extend(remaining_tags.iter().cloned());
                }
            } else {
                let placeholders = tags.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                conditions.push(format!(
                    "a.id IN (SELECT asset_id FROM asset_tags WHERE tag_id IN ({}))",
                    placeholders
                ));
                binds.extend(tags.clone());
            }
        }
    }

    // Collection filter: supports multi-collection, subcollections, logic "and" (in all), "or" (in any), "and_or" (primary AND any remaining), "none" (unorganized)
    let mut target_collections: Vec<String> = Vec::new();
    if let Some(col_id) = payload.query.collection_id.as_ref().or(f.collection_id.as_ref()) {
        if !col_id.is_empty() && !target_collections.contains(col_id) {
            target_collections.push(col_id.clone());
        }
    }
    if let Some(cols) = &f.collection_ids {
        for cid in cols {
            if !cid.is_empty() && !target_collections.contains(cid) {
                target_collections.push(cid.clone());
            }
        }
    }

    let col_logic = f.collection_logic.as_deref().unwrap_or("or");
    let include_sub = f.include_subcollections.unwrap_or_else(|| payload.query.include_subcollections.unwrap_or(true));

    if col_logic == "none" || f.unorganized == Some(true) {
        conditions.push("a.id NOT IN (SELECT asset_id FROM collection_assets)".into());
    } else if !target_collections.is_empty() {
        if col_logic == "and" {
            for cid in &target_collections {
                if include_sub {
                    conditions.push(
                        "a.id IN (
                            WITH RECURSIVE col_tree AS (
                                SELECT id FROM collections WHERE id = ?
                                UNION ALL
                                SELECT c.id FROM collections c JOIN col_tree ct ON c.parent_id = ct.id
                            )
                            SELECT asset_id FROM collection_assets WHERE collection_id IN (SELECT id FROM col_tree)
                        )".into()
                    );
                } else {
                    conditions.push(
                        "a.id IN (SELECT asset_id FROM collection_assets WHERE collection_id = ?)".into()
                    );
                }
                binds.push(cid.clone());
            }
        } else if col_logic == "and_or" {
            if target_collections.len() <= 1 {
                let cid = &target_collections[0];
                if include_sub {
                    conditions.push(
                        "a.id IN (
                            WITH RECURSIVE col_tree AS (
                                SELECT id FROM collections WHERE id = ?
                                UNION ALL
                                SELECT c.id FROM collections c JOIN col_tree ct ON c.parent_id = ct.id
                            )
                            SELECT asset_id FROM collection_assets WHERE collection_id IN (SELECT id FROM col_tree)
                        )".into()
                    );
                } else {
                    conditions.push(
                        "a.id IN (SELECT asset_id FROM collection_assets WHERE collection_id = ?)".into()
                    );
                }
                binds.push(cid.clone());
            } else {
                let primary_col = &target_collections[0];
                let remaining_cols = &target_collections[1..];
                if include_sub {
                    conditions.push(
                        "a.id IN (
                            WITH RECURSIVE col_tree AS (
                                SELECT id FROM collections WHERE id = ?
                                UNION ALL
                                SELECT c.id FROM collections c JOIN col_tree ct ON c.parent_id = ct.id
                            )
                            SELECT asset_id FROM collection_assets WHERE collection_id IN (SELECT id FROM col_tree)
                        )".into()
                    );
                    binds.push(primary_col.clone());

                    let rem_placeholders = remaining_cols.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    conditions.push(format!(
                        "a.id IN (
                            WITH RECURSIVE col_tree AS (
                                SELECT id FROM collections WHERE id IN ({})
                                UNION ALL
                                SELECT c.id FROM collections c JOIN col_tree ct ON c.parent_id = ct.id
                            )
                            SELECT asset_id FROM collection_assets WHERE collection_id IN (SELECT id FROM col_tree)
                        )",
                        rem_placeholders
                    ));
                    binds.extend(remaining_cols.iter().cloned());
                } else {
                    conditions.push(
                        "a.id IN (SELECT asset_id FROM collection_assets WHERE collection_id = ?)".into()
                    );
                    binds.push(primary_col.clone());

                    let rem_placeholders = remaining_cols.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    conditions.push(format!(
                        "a.id IN (SELECT asset_id FROM collection_assets WHERE collection_id IN ({}))",
                        rem_placeholders
                    ));
                    binds.extend(remaining_cols.iter().cloned());
                }
            }
        } else {
            if include_sub {
                let placeholders = target_collections.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                conditions.push(format!(
                    "a.id IN (
                        WITH RECURSIVE col_tree AS (
                            SELECT id FROM collections WHERE id IN ({})
                            UNION ALL
                            SELECT c.id FROM collections c JOIN col_tree ct ON c.parent_id = ct.id
                        )
                        SELECT asset_id FROM collection_assets WHERE collection_id IN (SELECT id FROM col_tree)
                    )",
                    placeholders
                ));
                binds.extend(target_collections);
            } else {
                let placeholders = target_collections.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                conditions.push(format!(
                    "a.id IN (SELECT asset_id FROM collection_assets WHERE collection_id IN ({}))",
                    placeholders
                ));
                binds.extend(target_collections);
            }
        }
    }

    // Smart filter conditions
    if let Some(dir) = &f.directory {
        if !dir.trim().is_empty() {
            conditions.push("a.file_path LIKE ?".into());
            binds.push(format!("%{}%", dir.trim().replace('\\', "/")));
        }
    }

    if let Some(prefix) = &f.name_prefix {
        if !prefix.trim().is_empty() {
            conditions.push("a.file_name LIKE ?".into());
            binds.push(format!("{}%", prefix.trim()));
        }
    }

    if let Some(suffix) = &f.name_suffix {
        if !suffix.trim().is_empty() {
            conditions.push("a.file_name LIKE ?".into());
            binds.push(format!("%{}", suffix.trim()));
        }
    }

    let (folders, folder_total) = if payload.result_scope.as_deref() == Some("files") {
        (Vec::new(), 0)
    } else {
        crate::folders::search(all_libs, &payload.query, limit, &conditions, &binds)
            .await.map_err(|e| e.to_string())?
    };
    if payload.result_scope.as_deref() == Some("folders") {
        return Ok(SearchResult { folders, folder_total, assets: Vec::new(), total: 0, page: payload.page, page_size: payload.page_size });
    }
    if let Some(path) = &payload.query.folder_path {
        let prefix = if path.is_empty() { String::new() } else { format!("{}/", path.trim_end_matches('/')) };
        if prefix.is_empty() {
            conditions.push("instr(replace(a.file_path,char(92),'/'),'/') = 0".into());
        } else {
            conditions.push(crate::folders::DIRECT_FILES.into());
            let upper = format!("{}0", &prefix[..prefix.len()-1]);
            binds.extend([prefix.clone(), upper, prefix]);
        }
    }
    let sort_col = match payload.query.sort.field.as_str() {
        "name" => "a.file_name", "size" => "a.size_bytes", "rating" => "a.rating",
        "kind" => "a.kind", "favorite" => "a.is_favorite", _ => "a.modified_at",
    };
    let sort_dir = if payload.query.sort.order == "asc" { "ASC" } else { "DESC" };
    let single = all_libs.len() == 1;
    let mut total = 0;
    let mut all_assets: Vec<(Asset, usize)> = Vec::new();
    for (library_index, lib) in all_libs.iter().enumerate() {
        let (mut text_conditions, text_binds) = crate::search_index::text_conditions(&payload.query.text, &lib.root_path.to_string_lossy(), true);
        text_conditions.extend(conditions.iter().cloned());
        let where_clause = if text_conditions.is_empty() { String::new() } else { format!("WHERE {}", text_conditions.join(" AND ")) };
        let query_binds: Vec<_> = text_binds.iter().chain(binds.iter()).collect();
        // Filtre önce uygulanır; gruplanan kartın temsilcisi yalnız eşleşen dosyalardan seçilir.
        let grouped = payload.query.group_related == Some(true) && f.extensions.as_ref().is_none_or(Vec::is_empty) && f.kinds.as_ref().is_none_or(Vec::is_empty);
        let source = if grouped && where_clause.is_empty() {
            format!("WITH representatives AS ({}) ", crate::workflow::GROUP_REPRESENTATIVES)
        } else if grouped {
            format!("WITH matched AS (SELECT a.id,w.group_key,w.group_rank FROM assets a LEFT JOIN asset_workflow w ON w.asset_id=a.id {where_clause}), ranked AS (SELECT id,row_number() OVER (PARTITION BY CASE WHEN EXISTS(SELECT 1 FROM asset_workflow g WHERE g.group_key=matched.group_key AND g.group_rank<3) THEN group_key ELSE id END ORDER BY coalesce(group_rank,4),id) AS group_position FROM matched), representatives AS (SELECT id FROM ranked WHERE group_position=1) ")
        } else { String::new() };
        let where_clause = if grouped { "WHERE a.id IN (SELECT id FROM representatives)".into() } else { where_clause };
        let count_sql = if grouped { format!("{source}SELECT COUNT(*) FROM representatives") } else { format!("SELECT COUNT(*) FROM assets a {where_clause}") };
        // Önce yalnız sayfanın kimliklerini seç; metadata ve etiketleri en fazla 500 dosya için oku.
        let sort_index = if grouped { match sort_col {
            "a.modified_at" => " INDEXED BY idx_assets_modified_at",
            "a.rating" => " INDEXED BY idx_assets_rating",
            "a.kind" => " INDEXED BY idx_assets_kind",
            "a.is_favorite" => " INDEXED BY idx_assets_is_favorite",
            _ => "",
        }} else { "" };
        let data_sql = format!(r#"{source}SELECT
            a.id, a.file_path, a.file_name, a.extension, a.kind,
            a.size_bytes, a.modified_at, a.indexed_at, a.rating,
            a.color_label, a.is_favorite, a.description, a.thumbnail_path,
            a.preview_status, a.metadata,
            (SELECT json_group_array(tag_id) FROM asset_tags WHERE asset_id = a.id) AS tags,
            (SELECT json_group_array(collection_id) FROM collection_assets WHERE asset_id = a.id) AS collections
            FROM assets a WHERE a.id IN (SELECT a.id FROM assets a{sort_index} {where_clause} ORDER BY {sort_col} {sort_dir},a.id LIMIT ? OFFSET ?)
            ORDER BY {sort_col} {sort_dir},a.id"#);
        let mut count = sqlx::query_scalar::<_, i64>(&count_sql);
        let mut data = sqlx::query_as::<_, RawAssetRow>(&data_sql);
        for value in query_binds { count = count.bind(value); data = data.bind(value); }
        total += count.fetch_one(&lib.pool).await.map_err(|e| e.to_string())?;
        let rows = data.bind(if single { limit } else { offset.saturating_add(limit) }).bind(if single { offset } else { 0 })
            .fetch_all(&lib.pool).await.map_err(|e| e.to_string())?;
        all_assets.extend(rows.into_iter().map(|row| (row_to_asset(Some(&lib.root_path), row), library_index)));
    }
    if !single {
        let is_asc = payload.query.sort.order == "asc";
        all_assets.sort_by(|(a, _), (b, _)| {
            let cmp = match payload.query.sort.field.as_str() {
                "name" => a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()),
                "size" => a.size_bytes.cmp(&b.size_bytes), "rating" => a.rating.cmp(&b.rating),
                "kind" => a.kind.cmp(&b.kind), "favorite" => a.is_favorite.cmp(&b.is_favorite),
                _ => a.modified_at.cmp(&b.modified_at),
            };
            if is_asc { cmp } else { cmp.reverse() }
        });
    }
    let mut page_entries: Vec<_> = all_assets.into_iter().skip(if single { 0 } else { offset as usize }).take(limit as usize).collect();
    for (library_index, lib) in all_libs.iter().enumerate() {
        let positions: Vec<_> = page_entries.iter().enumerate().filter_map(|(position, (_, index))| (*index == library_index).then_some(position)).collect();
        let mut assets: Vec<_> = positions.iter().map(|position| page_entries[*position].0.clone()).collect();
        attach_related_images(&lib.pool, &lib.root_path, &mut assets).await?;
        crate::workflow::enrich(lib, &mut assets).await.map_err(|e| e.to_string())?;
        for (position, asset) in positions.into_iter().zip(assets) { page_entries[position].0 = asset; }
    }
    Ok(SearchResult {
        folders, folder_total, assets: page_entries.into_iter().map(|(asset, _)| asset).collect(),
        total, page: payload.page, page_size: payload.page_size,
    })
}

#[tauri::command]
pub async fn get_asset(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Asset> {
    let libs = state.db.get_all_libraries().await;

    // Search across open libraries
    for lib in libs {
        let row = sqlx::query_as::<_, RawAssetRow>(
            r#"SELECT a.id, a.file_path, a.file_name, a.extension, a.kind,
               a.size_bytes, a.modified_at, a.indexed_at, a.rating,
               a.color_label, a.is_favorite, a.description, a.thumbnail_path,
               a.preview_status, a.metadata,
               (SELECT json_group_array(tag_id) FROM asset_tags WHERE asset_id = a.id) AS tags,
               (SELECT json_group_array(collection_id) FROM collection_assets WHERE asset_id = a.id) AS collections
               FROM assets a WHERE a.id = ?"#,
        )
        .bind(&id)
        .fetch_optional(&lib.pool)
        .await
        .map_err(|e| e.to_string())?;

        if let Some(r) = row {
            let mut assets = [row_to_asset(Some(&lib.root_path), r)];
            attach_related_images(&lib.pool, &lib.root_path, &mut assets).await?;
            crate::workflow::enrich(&lib, &mut assets).await.map_err(|e| e.to_string())?;
            return Ok(assets.into_iter().next().unwrap());
        }
    }

    // Fallback to active/default pool
    let fallback_pool = state.db.get_active_pool().await;
    let row = sqlx::query_as::<_, RawAssetRow>(
        r#"SELECT a.id, a.file_path, a.file_name, a.extension, a.kind,
           a.size_bytes, a.modified_at, a.indexed_at, a.rating,
           a.color_label, a.is_favorite, a.description, a.thumbnail_path,
           a.preview_status, a.metadata,
           (SELECT json_group_array(tag_id) FROM asset_tags WHERE asset_id = a.id) AS tags,
           (SELECT json_group_array(collection_id) FROM collection_assets WHERE asset_id = a.id) AS collections
           FROM assets a WHERE a.id = ?"#,
    )
    .bind(&id)
    .fetch_optional(&fallback_pool)
    .await
    .map_err(|e| e.to_string())?;

    match row {
        Some(r) => {
            let root_opt = state.db.get_library_root(None).await;
            Ok(row_to_asset(root_opt.as_deref(), r))
        }
        None => Err(format!("Asset not found: {}", id)),
    }
}

#[tauri::command]
pub async fn update_asset(
    id: String,
    patch: AssetPatch,
    state: State<'_, Arc<AppState>>,
) -> Result<Asset> {
    let libs = state.db.get_all_libraries().await;
    let mut pools_to_update: Vec<(sqlx::Pool<sqlx::Sqlite>, Option<std::path::PathBuf>)> = Vec::new();

    for lib in &libs {
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM assets WHERE id = ?")
            .bind(&id)
            .fetch_optional(&lib.pool)
            .await
            .unwrap_or(None);

        if exists.is_some() {
            pools_to_update.push((lib.pool.clone(), Some(lib.root_path.clone())));
        }
    }

    let master_exists: Option<(String,)> = sqlx::query_as("SELECT id FROM assets WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db.master_pool)
        .await
        .unwrap_or(None);

    if master_exists.is_some() || pools_to_update.is_empty() {
        pools_to_update.push((state.db.master_pool.clone(), state.db.get_library_root(None).await));
    }

    let active_pool = state.db.get_active_pool().await;
    pools_to_update.push((active_pool, state.db.get_library_root(None).await));

    for (pool, root) in &pools_to_update {
        if let Some(rating) = patch.rating {
            let _ = sqlx::query("UPDATE assets SET rating = ? WHERE id = ?")
                .bind(rating).bind(&id).execute(pool).await;
        }
        if let Some(ref color_label) = patch.color_label {
            let _ = sqlx::query("UPDATE assets SET color_label = ? WHERE id = ?")
                .bind(color_label).bind(&id).execute(pool).await;
        }
        if let Some(is_fav) = patch.is_favorite {
            let fav_int: i64 = if is_fav { 1 } else { 0 };
            let _ = sqlx::query("UPDATE assets SET is_favorite = ? WHERE id = ?")
                .bind(fav_int).bind(&id).execute(pool).await;
        }
        if let Some(ref description) = patch.description {
            let _ = sqlx::query("UPDATE assets SET description = ? WHERE id = ?")
                .bind(description).bind(&id).execute(pool).await;
        }
        if let Some(ref thumbnail_path) = patch.thumbnail_path {
            let rel_thumb = match root {
                Some(r) => resolve_path(r, thumbnail_path),
                None => thumbnail_path.clone(),
            };
            let _ = sqlx::query("UPDATE assets SET thumbnail_path = ? WHERE id = ?")
                .bind(rel_thumb).bind(&id).execute(pool).await;
        }
        if let Some(ref preview_status) = patch.preview_status {
            let _ = sqlx::query("UPDATE assets SET preview_status = ? WHERE id = ?")
                .bind(preview_status).bind(&id).execute(pool).await;
        }
        if let Some(ref tags) = patch.tags {
            let _ = sqlx::query("DELETE FROM asset_tags WHERE asset_id = ?")
                .bind(&id).execute(pool).await;
            for tag_id in tags {
                let tag_def = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
                    "SELECT id, name, color, parent_id FROM tags WHERE id = ?"
                )
                .bind(tag_id)
                .fetch_optional(&state.db.master_pool).await.unwrap_or(None);

                if let Some((tid, tname, tcolor, tparent)) = tag_def {
                    let _ = sqlx::query("INSERT OR IGNORE INTO tags (id, name, color, parent_id) VALUES (?, ?, ?, ?)")
                        .bind(&tid).bind(&tname).bind(&tcolor).bind(&tparent)
                        .execute(pool).await;
                } else {
                    let _ = sqlx::query("INSERT OR IGNORE INTO tags (id, name) VALUES (?, ?)")
                        .bind(tag_id).bind(tag_id)
                        .execute(pool).await;
                }

                let _ = sqlx::query("INSERT OR IGNORE INTO asset_tags (asset_id, tag_id) VALUES (?, ?)")
                    .bind(&id).bind(tag_id).execute(pool).await;
            }
        }
    }

    get_asset(id, state).await
}

#[tauri::command]
pub async fn delete_asset(
    id: String,
    delete_file: Option<bool>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let should_delete_file = delete_file.unwrap_or(true);
    let libs = state.db.get_all_libraries().await;
    let mut target_pools = Vec::new();
    let mut file_to_delete: Option<std::path::PathBuf> = None;

    for lib in &libs {
        let row: Option<(String,)> = sqlx::query_as("SELECT file_path FROM assets WHERE id = ?")
            .bind(&id)
            .fetch_optional(&lib.pool)
            .await
            .unwrap_or(None);

        if let Some((fp,)) = row {
            let abs_path = if Path::new(&fp).is_absolute() {
                std::path::PathBuf::from(fp)
            } else {
                lib.root_path.join(fp)
            };
            file_to_delete = Some(abs_path);
            target_pools.push(lib.pool.clone());
        }
    }

    target_pools.push(state.db.master_pool.clone());
    target_pools.push(state.db.get_active_pool().await);

    // If deleting from filesystem
    if should_delete_file {
        if let Some(ref path) = file_to_delete {
            if path.exists() {
                // Try moving to Recycle Bin first
                if let Err(trash_err) = trash::delete(path) {
                    tracing::warn!("Failed to trash {:?}, falling back to remove_file: {}", path, trash_err);
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }

    // Delete records from databases
    for pool in target_pools {
        let _ = sqlx::query("DELETE FROM asset_tags WHERE asset_id = ?").bind(&id).execute(&pool).await;
        let _ = sqlx::query("DELETE FROM collection_assets WHERE asset_id = ?").bind(&id).execute(&pool).await;
        let _ = sqlx::query("DELETE FROM preview_jobs WHERE asset_id = ?").bind(&id).execute(&pool).await;
        let _ = sqlx::query("DELETE FROM assets WHERE id = ?").bind(&id).execute(&pool).await;
        let _ = sqlx::query("UPDATE libraries SET asset_count = (SELECT COUNT(*) FROM assets)").execute(&pool).await;
    }

    let _ = state.app_handle.emit("library:updated", ());
    Ok(())
}

#[tauri::command]
pub async fn delete_assets(
    ids: Vec<String>,
    delete_file: Option<bool>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    for id in ids {
        let _ = delete_asset(id, delete_file, state.clone()).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn rename_asset(
    id: String,
    new_name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Asset> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err("Asset file name cannot be empty".into());
    }

    let invalid_chars = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    if trimmed.chars().any(|c| invalid_chars.contains(&c)) {
        return Err("File name contains invalid characters: \\ / : * ? \" < > |".into());
    }

    let libs = state.db.get_all_libraries().await;
    let mut found_lib = None;
    let mut current_fp = None;
    let mut current_ext = None;

    for lib in &libs {
        let row: Option<(String, String)> = sqlx::query_as("SELECT file_path, extension FROM assets WHERE id = ?")
            .bind(&id)
            .fetch_optional(&lib.pool)
            .await
            .unwrap_or(None);

        if let Some((fp, ext)) = row {
            found_lib = Some(lib.clone());
            current_fp = Some(fp);
            current_ext = Some(ext);
            break;
        }
    }

    let (lib, old_rel_fp, old_ext) = match (found_lib, current_fp, current_ext) {
        (Some(l), Some(fp), Some(ext)) => (l, fp, ext),
        _ => return Err(format!("Asset not found: {}", id)),
    };

    let old_abs_path = if Path::new(&old_rel_fp).is_absolute() {
        std::path::PathBuf::from(&old_rel_fp)
    } else {
        lib.root_path.join(&old_rel_fp)
    };

    let parent_dir = old_abs_path
        .parent()
        .ok_or_else(|| "Could not determine parent directory".to_string())?;

    // Determine new filename and extension
    let (final_name, new_ext) = if let Some((stem, ext)) = trimmed.rsplit_once('.') {
        if !stem.is_empty() && !ext.is_empty() {
            (trimmed.to_string(), ext.to_lowercase())
        } else {
            (format!("{}.{}", trimmed, old_ext), old_ext.clone())
        }
    } else {
        (format!("{}.{}", trimmed, old_ext), old_ext.clone())
    };

    let new_abs_path = parent_dir.join(&final_name);

    if new_abs_path.exists() && new_abs_path != old_abs_path {
        return Err(format!("A file named '{}' already exists in this folder", final_name));
    }

    // Rename file on disk
    if old_abs_path.exists() {
        std::fs::rename(&old_abs_path, &new_abs_path)
            .map_err(|e| format!("Failed to rename file on disk: {}", e))?;
    }

    let new_rel_fp = make_relative_path(&lib.root_path, &new_abs_path);
    let now = chrono::Utc::now().timestamp_millis();

    // Update in all relevant databases
    let pools_to_update = vec![lib.pool.clone(), state.db.master_pool.clone(), state.db.get_active_pool().await];
    for pool in pools_to_update {
        let _ = sqlx::query(
            "UPDATE assets SET file_name = ?, file_path = ?, extension = ?, modified_at = ? WHERE id = ?"
        )
        .bind(&final_name)
        .bind(&new_rel_fp)
        .bind(&new_ext)
        .bind(now)
        .bind(&id)
        .execute(&pool)
        .await;
    }

    let mut connection = lib.pool.acquire().await.map_err(|e| e.to_string())?;
    let kind = crate::models::AssetKind::from_extension(&new_ext);
    crate::workflow::upsert(&mut connection, &id, &new_rel_fp, &new_ext, kind.as_str()).await.map_err(|e| e.to_string())?;
    drop(connection);
    let _ = state.app_handle.emit("library:updated", ());
    get_asset(id, state).await
}
