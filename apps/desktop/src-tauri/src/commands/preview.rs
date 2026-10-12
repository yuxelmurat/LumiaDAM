use chrono::Utc;
use std::sync::Arc;
use tauri::State;

use crate::models::PreviewJob;
use crate::AppState;

type Result<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn request_preview(
    asset_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    for lib in state.db.get_all_libraries().await {
        for ids in asset_ids.chunks(500) {
            enqueue_previews(&lib.pool, ids, true)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    state.db.preview_wakeup.notify_waiters();
    Ok(())
}

#[tauri::command]
pub async fn prioritize_previews(
    asset_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    if asset_ids.len() > 500 {
        return Err("At most 500 visible assets can be prioritized".into());
    }
    for lib in state.db.get_all_libraries().await {
        enqueue_previews(&lib.pool, &asset_ids, false)
            .await
            .map_err(|error| error.to_string())?;
    }
    state.db.preview_wakeup.notify_waiters();
    Ok(())
}

/// Toplu istek mevcut işi korur; yalnız açık yeniden deneme hatalı işleri kuyruğa alır.
pub(crate) async fn enqueue_previews(
    pool: &sqlx::SqlitePool,
    ids: &[String],
    retry_errors: bool,
) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let ids_json = serde_json::to_string(ids)?;
    let now = Utc::now().timestamp_millis();
    let mut tx = pool.begin().await?;
    // İlk yazma transaction kilidini alır; eşzamanlı istekler çift iş oluşturamaz.
    sqlx::query("UPDATE preview_jobs SET priority=? WHERE status IN ('queued','pending') AND asset_id IN (SELECT value FROM json_each(?))")
        .bind(now).bind(&ids_json).execute(&mut *tx).await?;
    let candidates: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id,extension,preview_status,thumbnail_path FROM assets WHERE id IN (SELECT value FROM json_each(?))")
        .bind(&ids_json).fetch_all(&mut *tx).await?;
    // Eski ortak küçük resim veya kaybolan yerel cache yeniden üretilir; kaynak dosya açılmaz.
    let candidates = tokio::task::spawn_blocking(move || candidates.into_iter().filter_map(|(id, extension, status, thumb)| {
        if !crate::preview::is_previewable(&extension) { return None; }
        let invalid = status == "done" && (thumb.as_ref().is_some_and(|path| crate::db::is_legacy_thumbnail(path)) ||
            (crate::preview::is_native_image(&extension) && thumb.as_ref().is_none_or(|path| !std::path::Path::new(path).is_file())));
        (status != "done" || invalid).then_some((id, invalid))
    }).collect::<Vec<_>>()).await?;
    for (id, invalid) in &candidates {
        if !invalid { continue; }
        // Çalışan iş korunur; yalnız tamamlanmış eski çıktının kimliği değiştirilir.
        sqlx::query("DELETE FROM preview_jobs WHERE asset_id=? AND status='done'").bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE assets SET preview_status='pending',thumbnail_path=NULL WHERE id=?").bind(id).execute(&mut *tx).await?;
    }
    let eligible: Vec<_> = candidates.into_iter()
        .map(|(asset_id,_)| serde_json::json!({"asset_id":asset_id,"job_id":uuid::Uuid::new_v4().to_string()}))
        .collect();
    let eligible_json = serde_json::to_string(&eligible)?;
    if retry_errors {
        sqlx::query("UPDATE preview_jobs SET status='queued',lane='auto',priority=?,error_message=NULL
            WHERE status='error' AND asset_id IN (SELECT json_extract(value,'$.asset_id') FROM json_each(?))
            AND NOT EXISTS (SELECT 1 FROM preview_jobs active WHERE active.asset_id=preview_jobs.asset_id AND active.status IN ('processing','queued','pending','done'))")
            .bind(now).bind(&eligible_json).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO preview_jobs(id,asset_id,status,created_at,priority)
        SELECT json_extract(entry.value,'$.job_id'),json_extract(entry.value,'$.asset_id'),'queued',?,?
        FROM json_each(?) entry WHERE NOT EXISTS
        (SELECT 1 FROM preview_jobs WHERE asset_id=json_extract(entry.value,'$.asset_id'))")
        .bind(now).bind(now).bind(&eligible_json).execute(&mut *tx).await?;
    sqlx::query("UPDATE assets SET preview_status='pending'
        WHERE id IN (SELECT json_extract(value,'$.asset_id') FROM json_each(?))
        AND EXISTS (SELECT 1 FROM preview_jobs WHERE asset_id=assets.id AND status IN ('queued','pending'))")
        .bind(&eligible_json).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[tauri::command]
pub async fn get_preview_jobs(state: State<'_, Arc<AppState>>) -> Result<Vec<PreviewJob>> {
    let libs = state.db.get_all_libraries().await;
    let mut all_jobs = Vec::new();

    for lib in &libs {
        if let Ok(rows) = sqlx::query_as::<_, (String, String, String, Option<i64>, Option<String>, i64)>(
            "SELECT id, asset_id, status, progress, error_message, created_at FROM preview_jobs ORDER BY created_at DESC LIMIT 50"
        )
        .fetch_all(&lib.pool)
        .await {
            for (id, asset_id, status, progress, error_message, created_at) in rows {
                all_jobs.push(PreviewJob { id, asset_id, status, progress, error_message, created_at });
            }
        }
    }

    all_jobs.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    all_jobs.truncate(100);
    Ok(all_jobs)
}
