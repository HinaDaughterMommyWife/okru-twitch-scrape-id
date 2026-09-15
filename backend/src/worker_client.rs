//! Worker API client: per-slug profile, live ids and VOD catalog.

use crate::vk::CatalogItem;
use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};

const WORKER_USER_AGENT: &str = "okru-vk-stream-check/1.0 (+https://vk.com)";
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Public data the web needs for `/[slug]`. Whitelist is never published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicProfile {
    pub slug: String,
    pub display_name: String,
    pub twitch_channel: String,
    pub idvk: String,
    /// First alias, shown on the "no stream" page.
    pub command: String,
}

#[derive(Serialize)]
struct StreamBody<'a> {
    vk_oid: &'a str,
    vk_id: &'a str,
    user: &'a PublicProfile,
}

#[derive(Serialize)]
struct VodsBody<'a> {
    items: &'a [CatalogItem],
}

#[derive(Deserialize)]
struct UsersList {
    slugs: Vec<String>,
}

#[derive(Clone)]
pub struct WorkerClient {
    http: reqwest::Client,
    base: String,
    auth: String,
}

impl WorkerClient {
    pub fn new(http: reqwest::Client, base: String, post_auth: &str) -> Self {
        let auth = format!("Basic {}", STANDARD.encode(format!("admin:{post_auth}")));
        Self { http, base, auth }
    }

    fn url(&self, slug: &str, suffix: &str) -> String {
        format!("{}/users/{}{suffix}", self.base, urlencoding::encode(slug))
    }

    /// Slugs of every profile stored in the worker.
    pub async fn list_users(&self) -> Result<Vec<String>> {
        let url = format!("{}/users", self.base);
        let resp = self
            .request(Method::GET, &url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Worker GET {url} HTTP {status}: {text}");
        }
        Ok(resp.json::<UsersList>().await.context("users list JSON")?.slugs)
    }

    pub async fn put_user(&self, profile: &PublicProfile) -> Result<()> {
        let url = self.url(&profile.slug, "");
        self.send(Method::PUT, &url, Some(profile)).await?;
        tracing::info!("Worker PUT user slug={}", profile.slug);
        Ok(())
    }

    pub async fn delete_user(&self, slug: &str) -> Result<()> {
        let url = self.url(slug, "");
        self.send::<()>(Method::DELETE, &url, None).await?;
        tracing::info!("Worker DELETE user slug={slug}");
        Ok(())
    }

    /// Live ids + profile (instantiates the page if it did not exist yet).
    pub async fn post_stream(&self, profile: &PublicProfile, vk_oid: &str, vk_id: &str) -> Result<()> {
        let url = self.url(&profile.slug, "/streaming");
        tracing::info!("Worker POST slug={} vk_oid={vk_oid} vk_id={vk_id}", profile.slug);
        let body = StreamBody { vk_oid, vk_id, user: profile };
        self.send(Method::POST, &url, Some(&body)).await?;
        Ok(())
    }

    pub async fn post_vods(&self, slug: &str, items: &[CatalogItem]) -> Result<()> {
        let url = self.url(slug, "/vods");
        self.send(Method::POST, &url, Some(&VodsBody { items })).await?;
        tracing::info!("Worker POST vods slug={slug} items={}", items.len());
        Ok(())
    }

    async fn send<B: Serialize>(&self, method: Method, url: &str, body: Option<&B>) -> Result<()> {
        let is_delete = method == Method::DELETE;
        let mut req = self.request(method.clone(), url);
        if let Some(body) = body {
            req = req.json(body);
        }

        let resp = req.send().await.with_context(|| format!("{method} {url}"))?;
        let status = resp.status();
        // Deleting something that is already gone is fine.
        if status.is_success() || (is_delete && status == StatusCode::NOT_FOUND) {
            return Ok(());
        }
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Worker {method} {url} HTTP {status}: {text}")
    }

    fn request(&self, method: Method, url: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .header("Authorization", &self.auth)
            .header("Accept", "application/json")
            .header("Cache-Control", "no-store")
            .header("User-Agent", WORKER_USER_AGENT)
            .timeout(TIMEOUT)
    }
}
