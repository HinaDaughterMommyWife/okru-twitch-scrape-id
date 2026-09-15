//! One VK scrape for a user → worker `/vods` + `/streaming`. Shared by chat command and interval loop.

use anyhow::Result;

use crate::registry::UserEntry;
use crate::vk;
use crate::worker_client::WorkerClient;

/// Returns `true` when a live stream was found.
pub async fn run_check(http: &reqwest::Client, worker: &WorkerClient, entry: &UserEntry) -> Result<bool> {
    let slug = &entry.user.slug;
    let result = vk::prefetch(http, &entry.user.idvk).await?;
    tracing::info!(
        "[{slug}] scrape found={} oid={} vid={} vods={}",
        result.found,
        result.vk_oid,
        result.vk_id,
        result.items.len()
    );
    if let Err(e) = worker.post_vods(slug, &result.items).await {
        tracing::error!("[{slug}] VODS POST failed (non-fatal): {e:#}");
    }
    worker
        .post_stream(&entry.profile(), &result.vk_oid, &result.vk_id)
        .await?;
    Ok(result.found)
}
