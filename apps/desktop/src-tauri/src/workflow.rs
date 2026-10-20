use crate::{
    db::{resolve_path, LibraryHandle},
    models::{Asset, RelatedImage},
};
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};
use std::{collections::HashMap, path::PathBuf};

// Açılış görünümünde tam dosya kayıtlarını sıralamadan grup temsilcilerini indekslerden çıkarırız.
pub(crate) const GROUP_REPRESENTATIVES: &str = "SELECT (SELECT asset_id FROM asset_workflow g WHERE g.group_key=w.group_key ORDER BY group_rank,asset_id LIMIT 1) AS id FROM asset_workflow w GROUP BY group_key HAVING min(group_rank)<3 UNION ALL SELECT w.asset_id FROM asset_workflow w WHERE w.group_rank>=3 AND NOT EXISTS(SELECT 1 FROM asset_workflow g WHERE g.group_key=w.group_key AND g.group_rank<3)";

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PathLabels {
    pub year: String,
    pub source: String,
    pub room: String,
    pub renderer: String,
}

/// Belirsiz klasör adlarını tahmin etmeyiz; yalnız bilinen sözcükler ve yıllar etiketlenir.
pub fn infer_labels(path: &str) -> PathLabels {
    let path = path.replace('\\', "/");
    let directory = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let mut labels = PathLabels::default();
    for component in directory.split('/') {
        let words: Vec<_> = component
            .split(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
            .collect();
        let has = |names: &[&str]| {
            names
                .iter()
                .any(|name| words.iter().any(|word| word == name))
        };
        for word in &words {
            if word.len() == 4
                && word
                    .parse::<u32>()
                    .is_ok_and(|n| (1900..=2100).contains(&n))
            {
                labels.year = word.clone();
            }
        }
        if has(&["3dsky", "3ddd"]) {
            labels.source = "3dsky".into();
        }
        if has(&["evermotion"]) {
            labels.source = "Evermotion".into();
        }
        if has(&["poligon", "poliigon"]) {
            labels.source = "Poliigon".into();
        }
        if has(&["quixel", "megascans"]) {
            labels.source = "Quixel".into();
        }
        if has(&["corona"]) {
            labels.renderer = "Corona".into();
        }
        if has(&["vray"]) || words.windows(2).any(|pair| pair == ["v", "ray"]) {
            labels.renderer = "V-Ray".into();
        }
        if has(&["fstorm"]) {
            labels.renderer = "FStorm".into();
        }
        if has(&["cycles"]) {
            labels.renderer = "Cycles".into();
        }
        if has(&["kitchen", "mutfak"]) {
            labels.room = "Kitchen".into();
        }
        if has(&["salon"]) || words.windows(2).any(|pair| pair == ["living", "room"]) {
            labels.room = "Living room".into();
        }
        if has(&["bedroom"]) || words.windows(2).any(|pair| pair == ["yatak", "odası"]) {
            labels.room = "Bedroom".into();
        }
        if has(&["bathroom", "banyo"]) {
            labels.room = "Bathroom".into();
        }
        if has(&["office", "ofis"]) {
            labels.room = "Office".into();
        }
        if has(&["exterior"]) {
            labels.room = "Exterior".into();
        }
        if has(&["interior"]) && labels.room.is_empty() {
            labels.room = "Interior".into();
        }
        if words.windows(2).any(|pair| pair == ["dining", "room"]) {
            labels.room = "Dining room".into();
        }
    }
    labels
}

pub fn group_key(path: &str, extension: &str, kind: &str) -> (String, String, i64) {
    let extension = extension.to_ascii_lowercase();
    let normalized = path.replace('\\', "/").to_lowercase();
    let (directory, name) = normalized.rsplit_once('/').unwrap_or(("", &normalized));
    let mut stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    let image = matches!(
        extension.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp"
    );
    if image {
        for suffix in ["_preview", "-preview", "_render", "-render"] {
            if let Some(base) = stem.strip_suffix(suffix).filter(|base| !base.is_empty()) {
                stem = base;
                break;
            }
        }
    }
    let rank = if kind == "3d_model" {
        0
    } else if matches!(extension.as_str(), "zip" | "rar" | "7z") {
        1
    } else if kind == "material" {
        2
    } else if image {
        3
    } else {
        4
    };
    // Rastgele adlandırılmış diğer belgeleri yalnız aynı gövde var diye bir modele bağlamayız.
    let stem = if rank == 4 { name } else { stem };
    (directory.into(), format!("{directory}/{stem}"), rank)
}

pub async fn upsert(
    connection: &mut SqliteConnection,
    id: &str,
    path: &str,
    extension: &str,
    kind: &str,
) -> anyhow::Result<()> {
    let (directory, key, rank) = group_key(path, extension, kind);
    let inferred = infer_labels(path);
    let overrides: Option<String> =
        sqlx::query_scalar("SELECT overrides FROM asset_workflow WHERE asset_id=?")
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?;
    let overrides: HashMap<String, String> = overrides
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default();
    let value = |key: &str, automatic: &str| {
        overrides
            .get(key)
            .cloned()
            .unwrap_or_else(|| automatic.into())
    };
    sqlx::query("INSERT INTO asset_workflow(asset_id,file_path,directory,group_key,group_rank,year,source,room,renderer,inferred) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(asset_id) DO UPDATE SET file_path=excluded.file_path,directory=excluded.directory,group_key=excluded.group_key,group_rank=excluded.group_rank,year=excluded.year,source=excluded.source,room=excluded.room,renderer=excluded.renderer,inferred=excluded.inferred")
        .bind(id).bind(path).bind(directory).bind(key).bind(rank)
        .bind(value("year", &inferred.year)).bind(value("source", &inferred.source))
        .bind(value("room", &inferred.room)).bind(value("renderer", &inferred.renderer))
        .bind(serde_json::to_string(&inferred)?).execute(connection).await?;
    Ok(())
}

/// Mevcut dosyalar yerel katalogdan aktarılır; kaynak sürücüye erişilmez.
pub async fn initialize(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(include_str!("../migrations/006_workflows.sql"))
        .execute(pool)
        .await?;
    let mut tx = pool.begin().await?;
    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as("SELECT a.id,a.file_path,a.extension,a.kind,w.asset_id IS NOT NULL FROM assets a LEFT JOIN asset_workflow w ON w.asset_id=a.id WHERE w.asset_id IS NULL OR w.file_path<>a.file_path")
        .fetch_all(&mut *tx).await?;
    // İlk geçişte dosya başına iki sorgu yerine 80 kayıtlık toplu ekleme yap.
    let missing: Vec<_> = rows.iter().filter(|row| !row.4).collect();
    for chunk in missing.chunks(80) {
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("INSERT INTO asset_workflow(asset_id,file_path,directory,group_key,group_rank,year,source,room,renderer,inferred) ");
        query.push_values(chunk, |mut values, row| {
            let (id, path, ext, kind, _) = *row;
            let (directory, key, rank) = group_key(path, ext, kind);
            let labels = infer_labels(path);
            let inferred = serde_json::to_string(&labels).unwrap();
            values
                .push_bind(id)
                .push_bind(path)
                .push_bind(directory)
                .push_bind(key)
                .push_bind(rank)
                .push_bind(labels.year)
                .push_bind(labels.source)
                .push_bind(labels.room)
                .push_bind(labels.renderer)
                .push_bind(inferred);
        });
        query
            .push(" ON CONFLICT(asset_id) DO NOTHING")
            .build()
            .execute(&mut *tx)
            .await?;
    }
    for (id, path, ext, kind, existed) in &rows {
        if *existed {
            upsert(&mut tx, id, path, ext, kind).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupFile {
    pub id: String,
    pub file_name: String,
    pub file_path: String,
    pub extension: String,
    pub kind: String,
    pub thumbnail_path: Option<String>,
}

pub fn offline_path(library: &LibraryHandle, id: &str, name: &str) -> anyhow::Result<PathBuf> {
    uuid::Uuid::parse_str(id)?;
    anyhow::ensure!(
        !name.is_empty() && !name.contains(['/', '\\', '\0', ':']) && name != "." && name != "..",
        "Invalid cached file name"
    );
    Ok(library.catalog_dir.join("offline").join(id).join(name))
}

/// Sayfadaki kayıtlar toplu okunur; önizleme kaynağının erişilebilirliği burada denetlenmez.
pub async fn enrich(library: &LibraryHandle, assets: &mut [Asset]) -> anyhow::Result<()> {
    type Enriched = (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let mut rows_by_id = HashMap::new();
    let mut groups: HashMap<String, Vec<GroupFile>> = HashMap::new();
    for chunk in assets.chunks(400) {
        let mut query=sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT w.asset_id,w.year,w.source,w.room,w.renderer,w.inferred,w.overrides,w.related_mode,w.related_id,i.file_path,i.file_name,i.thumbnail_path,i.id,o.file_name,o.size_bytes,o.modified_at FROM asset_workflow w LEFT JOIN assets i ON i.id=w.related_id AND lower(i.extension) IN ('jpg','jpeg','png','webp') LEFT JOIN offline_assets o ON o.asset_id=w.asset_id WHERE w.asset_id IN (");
        let mut ids = query.separated(",");
        for asset in chunk {
            ids.push_bind(&asset.id);
        }
        ids.push_unseparated(")");
        for row in query
            .build_query_as::<Enriched>()
            .fetch_all(&library.pool)
            .await?
        {
            rows_by_id.insert(row.0.clone(), row);
        }
        let mut query=sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT target.asset_id,a.id,a.file_name,a.file_path,a.extension,a.kind,a.thumbnail_path FROM asset_workflow target JOIN asset_workflow sibling ON sibling.group_key=target.group_key JOIN assets a ON a.id=sibling.asset_id WHERE target.asset_id IN (");
        let mut ids = query.separated(",");
        for asset in chunk {
            ids.push_bind(&asset.id);
        }
        ids.push_unseparated(") ORDER BY sibling.group_rank,a.file_name COLLATE NOCASE,a.id");
        let rows: Vec<(
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
        )> = query.build_query_as().fetch_all(&library.pool).await?;
        for (target, id, name, path, extension, kind, thumb) in rows {
            groups.entry(target).or_default().push(GroupFile {
                id,
                file_name: name,
                file_path: resolve_path(&library.root_path, &path),
                extension,
                kind,
                thumbnail_path: thumb
                    .filter(|p| !crate::db::is_legacy_thumbnail(p))
                    .map(|p| resolve_path(&library.root_path, &p)),
            });
        }
    }
    for asset in assets {
        asset.library_id = Some(library.id.clone());
        asset.group_files = groups.remove(&asset.id).unwrap_or_default();
        if let Some((
            _,
            year,
            source,
            room,
            renderer,
            inferred,
            overrides,
            mode,
            _,
            path,
            name,
            thumb,
            image_id,
            cached_name,
            cached_size,
            cached_modified,
        )) = rows_by_id.remove(&asset.id)
        {
            asset.path_labels = PathLabels {
                year,
                source,
                room,
                renderer,
            };
            asset.inferred_labels = serde_json::from_str(&inferred).unwrap_or_default();
            asset.label_overrides = serde_json::from_str(&overrides).unwrap_or_default();
            asset.related_mode = mode.clone();
            if mode != "auto" {
                asset.related_image = None;
                if mode == "pinned" {
                    if let (Some(path), Some(name), Some(id)) = (path, name, image_id) {
                        asset.related_image = Some(RelatedImage {
                            id,
                            file_path: resolve_path(&library.root_path, &path),
                            file_name: name,
                            thumbnail_path: thumb
                                .filter(|p| !crate::db::is_legacy_thumbnail(p))
                                .map(|p| resolve_path(&library.root_path, &p)),
                        });
                    }
                }
            }
            if let Some(name) = cached_name {
                asset.offline_path = Some(
                    offline_path(library, &asset.id, &name)?
                        .to_string_lossy()
                        .into_owned(),
                );
                asset.offline_stale = cached_size != Some(asset.size_bytes)
                    || cached_modified != Some(asset.modified_at);
            }
        }
    }
    Ok(())
}

pub async fn locate(
    db: &crate::db::Database,
    id: &str,
) -> anyhow::Result<(LibraryHandle, String, String, i64, i64)> {
    uuid::Uuid::parse_str(id)?;
    for library in db.get_all_libraries().await {
        let row: Option<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT file_path,file_name,size_bytes,modified_at FROM assets WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&library.pool)
        .await?;
        if let Some((path, name, size, modified)) = row {
            return Ok((library, path, name, size, modified));
        }
    }
    anyhow::bail!("Asset not found in an open library")
}

pub async fn readable_path(db: &crate::db::Database, id: &str) -> anyhow::Result<PathBuf> {
    let (library, relative, _, size, modified) = locate(db, id).await?;
    if let Some(path) = cached_source(&library, id, size, modified).await? {
        return Ok(path);
    }
    source_path(db, &library, &relative).await
}

pub async fn cached_source(
    library: &LibraryHandle,
    id: &str,
    size: i64,
    modified: i64,
) -> anyhow::Result<Option<PathBuf>> {
    let cached: Option<(String, i64, i64)> = sqlx::query_as(
        "SELECT file_name,size_bytes,modified_at FROM offline_assets WHERE asset_id=?",
    )
    .bind(id)
    .fetch_optional(&library.pool)
    .await?;
    // Yerel kopya varsa NAS'a tek bir metadata isteği bile gönderilmez.
    if let Some((name, cached_size, cached_modified)) = cached {
        if size == cached_size && modified == cached_modified {
            let path = offline_path(library, id, &name)?;
            if tokio::fs::metadata(&path)
                .await
                .is_ok_and(|meta| meta.is_file() && meta.len() == size as u64)
            {
                let canonical = tokio::fs::canonicalize(&path).await?;
                anyhow::ensure!(
                    canonical.starts_with(
                        tokio::fs::canonicalize(library.catalog_dir.join("offline")).await?
                    ),
                    "Cached file is outside local storage"
                );
                return Ok(Some(canonical));
            }
        }
    }
    Ok(None)
}

pub async fn source_path(
    db: &crate::db::Database,
    library: &LibraryHandle,
    relative: &str,
) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(resolve_path(&library.root_path, relative));
    let canonical = tokio::fs::canonicalize(&path).await.map_err(|_| {
        anyhow::anyhow!(
            "Source is unavailable. Connect the library or keep this file available offline."
        )
    })?;
    let exports = library.catalog_dir.join("exports");
    if crate::storage::path_is_within(&path, &exports) {
        anyhow::ensure!(
            canonical.starts_with(tokio::fs::canonicalize(&exports).await?)
                && tokio::fs::metadata(&canonical).await?.is_file(),
            "Export resolves outside local storage"
        );
        return Ok(canonical);
    }
    let mut allowed = false;
    for root in crate::watcher::source_roots(db, library).await? {
        if !crate::storage::path_is_within(&path, &root) {
            continue;
        }
        if crate::storage::path_is_within(&canonical, &tokio::fs::canonicalize(root).await?) {
            allowed = true;
            break;
        }
    }
    anyhow::ensure!(
        allowed && tokio::fs::metadata(&canonical).await?.is_file(),
        "Source is outside the library or unavailable"
    );
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn catalog_backfill_batches_and_preserves_existing_label_corrections() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE assets(id TEXT PRIMARY KEY,file_path TEXT,extension TEXT,kind TEXT); CREATE TABLE search_revision(id INTEGER PRIMARY KEY,revision INTEGER); INSERT INTO search_revision VALUES(1,0); WITH RECURSIVE numbers(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM numbers WHERE n<160) INSERT INTO assets SELECT cast(n AS TEXT),'2023/kitchen/'||n||'.jpg','jpg','texture' FROM numbers")
            .execute(&pool).await.unwrap();
        initialize(&pool).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM asset_workflow WHERE room='Kitchen'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            161
        );
        sqlx::query("UPDATE asset_workflow SET overrides='{\"room\":\"Office\"}' WHERE asset_id='0'; UPDATE assets SET file_path='2025/bedroom/0.jpg' WHERE id='0'").execute(&pool).await.unwrap();
        initialize(&pool).await.unwrap();
        let row: (String, String, String) =
            sqlx::query_as("SELECT year,room,inferred FROM asset_workflow WHERE asset_id='0'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((row.0, row.1), ("2025".into(), "Office".into()));
        assert!(row.2.contains("Bedroom"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM asset_workflow")
                .fetch_one(&pool)
                .await
                .unwrap(),
            161
        );
        pool.close().await;
    }
    #[test]
    fn labels_use_directory_boundaries_and_deepest_specific_room() {
        assert_eq!(
            infer_labels("archive/2023/3dsky/interior/kitchen/corona/654.zip"),
            PathLabels {
                year: "2023".into(),
                source: "3dsky".into(),
                room: "Kitchen".into(),
                renderer: "Corona".into()
            }
        );
        assert_eq!(
            infer_labels("archive/2023/2025/salon/V-Ray/9.jpg").year,
            "2025"
        );
        assert_eq!(
            infer_labels("unknown/kitchen123/coronary/20235/9.jpg"),
            PathLabels::default()
        );
        assert_eq!(infer_labels("kitchen.jpg"), PathLabels::default());
    }
    #[test]
    fn groups_preserve_numbered_stems_and_folder_boundaries() {
        assert_eq!(
            group_key("kitchen/9.zip", "zip", "other").1,
            group_key("kitchen/9_render.jpg", "jpg", "texture").1
        );
        assert_ne!(
            group_key("kitchen/9.zip", "zip", "other").1,
            group_key("bedroom/9.jpg", "jpg", "texture").1
        );
        assert_ne!(
            group_key("kitchen/9.zip", "zip", "other").1,
            group_key("kitchen/19.jpg", "jpg", "texture").1
        );
        assert_ne!(
            group_key("kitchen/9.zip", "zip", "other").1,
            group_key("kitchen/9.txt", "txt", "document").1
        );
    }
}
