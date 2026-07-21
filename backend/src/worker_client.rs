//! POST {vk_oid, vk_id} to the Cloudflare Worker /streaming endpoint.

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Serialize;

const WORKER_USER_AGENT: &str = "okru-vk-stream-check/1.0 (+https://vk.com)";

#[derive(Debug, Serialize)]
struct PostBody<'a> {
    vk_oid: &'a str,
    vk_id: &'a str,
}

pub async fn post_to_worker(
    client: &reqwest::Client,
    post_url: &str,
    post_auth: &str,
    vk_oid: &str,
    vk_id: &str,
) -> Result<serde_json::Value> {
    let credentials = STANDARD.encode(format!("admin:{post_auth}"));
    let auth = format!("Basic {credentials}");

    tracing::info!("Worker POST vk_oid={vk_oid} vk_id={vk_id} url={post_url}");

    let resp = client
        .post(post_url)
        .header("Authorization", auth)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("Cache-Control", "no-store")
        .header("User-Agent", WORKER_USER_AGENT)
        .json(&PostBody { vk_oid, vk_id })
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("POST worker")?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    let payload: serde_json::Value = serde_json::from_str(&body_text)
        .unwrap_or_else(|_| serde_json::json!({ "raw": body_text }));

    if !status.is_success() {
        anyhow::bail!("Worker HTTP {status}: {payload}");
    }

    tracing::info!("Worker resp HTTP {status} body={payload}");
    Ok(payload)
}
