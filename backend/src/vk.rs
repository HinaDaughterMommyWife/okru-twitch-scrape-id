//! VK profile scrape via `window.cur.apiPrefetchCache` (`video.get`).
//! Live = `live_status == "started"`. Chrome UA required (Googlebot gets the old page).

#[path = "models/mod.rs"]
mod models;

use anyhow::{bail, Context, Result};
use models::VideoGetResponse;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, REFERER, USER_AGENT,
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

const CHROME_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const GOOGLEBOT_UA: &str =
    "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MARKER: &[u8] = br#""apiPrefetchCache":"#;

pub const NOT_FOUND_OID: &str = "NOT_FOUND";
pub const NOT_FOUND_VID: &str = "NOT_FOUND";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: i64,
    pub owner_id: i64,
    pub title: String,
    pub duration: String,
    pub live_status: String,
    pub date: i64,
    pub thumb: String,
}

#[derive(Debug, Clone)]
pub struct PrefetchResult {
    pub found: bool,
    pub vk_oid: String,
    pub vk_id: String,
    pub items: Vec<CatalogItem>,
}

pub fn normalize_url(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with("http") {
        return raw.to_string();
    }
    if raw.starts_with("id") || raw.chars().all(|c| c.is_ascii_digit()) {
        let slug = if raw.starts_with("id") {
            raw.to_string()
        } else {
            format!("id{raw}")
        };
        return format!("https://vk.com/{slug}");
    }
    format!("https://vk.com/{}", raw.trim_start_matches('/'))
}

fn vk_page_headers(page_url: &str, googlebot: bool) -> HeaderMap {
    let mut h = HeaderMap::new();
    if googlebot {
        h.insert(USER_AGENT, HeaderValue::from_static(GOOGLEBOT_UA));
        h.insert("From", HeaderValue::from_static("googlebot(at)googlebot.com"));
    } else {
        h.insert(USER_AGENT, HeaderValue::from_static(CHROME_UA));
    }
    h.insert(
        ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
        ),
    );
    h.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static("es-ES,es;q=0.9,en-US;q=0.8,en;q=0.7,ru;q=0.6"),
    );
    h.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(REFERER, HeaderValue::from_static("https://vk.com/"));
    h.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
    h.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
    let site = if page_url.contains("vk.com") || page_url.contains("vk.ru") {
        "same-origin"
    } else {
        "none"
    };
    h.insert("Sec-Fetch-Site", HeaderValue::from_static(site));
    h.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
    h.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));
    h.insert("Pragma", HeaderValue::from_static("no-cache"));
    h
}

pub async fn fetch_profile_bytes(
    client: &reqwest::Client,
    idvk: &str,
    googlebot: bool,
) -> Result<(String, Vec<u8>)> {
    let url = normalize_url(idvk);
    tracing::info!(
        "GET {url} ua={}",
        if googlebot { "googlebot" } else { "chrome" }
    );
    let resp = client
        .get(&url)
        .headers(vk_page_headers(&url, googlebot))
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error for {url}"))?;
    let final_url = resp.url().to_string();
    let bytes = resp.bytes().await.context("leer body VK")?;
    tracing::info!("VK fetch OK url={final_url} bytes={}", bytes.len());
    Ok((final_url, bytes.to_vec()))
}

pub fn extract_prefetch_value(bytes: &[u8]) -> Result<(Value, usize)> {
    let mut from = 0;
    let mut last_json_at: Option<usize> = None;

    while let Some(rel) = find_subslice(&bytes[from..], MARKER) {
        let abs = from + rel;
        let after = abs + MARKER.len();
        let trimmed = skip_ws(&bytes[after..]);
        let json_at = after + (bytes[after..].len() - trimmed.len());
        if trimmed.first() == Some(&b'[') {
            last_json_at = Some(json_at);
        }
        from = after;
    }

    let json_at = last_json_at.context(
        "no apiPrefetchCache JSON in HTML (wrong UA? Googlebot gets the old page)",
    )?;

    let mut de = serde_json::Deserializer::from_slice(&bytes[json_at..]);
    let value = Value::deserialize(&mut de).with_context(|| {
        let hint = String::from_utf8_lossy(&bytes[json_at..json_at.saturating_add(80)]);
        format!("invalid apiPrefetchCache JSON near: {hint}")
    })?;
    if !value.is_array() {
        bail!("apiPrefetchCache is not a JSON array");
    }
    Ok((value, json_at))
}

pub fn catalog_from_prefetch(prefetch: &Value) -> Result<PrefetchResult> {
    let entries = prefetch
        .as_array()
        .context("apiPrefetchCache is not an array")?;
    let video = entries
        .iter()
        .find(|e| e.get("method").and_then(Value::as_str) == Some("video.get"))
        .cloned()
        .context("video.get missing from apiPrefetchCache")?;

    let parsed: VideoGetResponse = serde_json::from_value(video)
        .context("map video.get into VideoGetResponse")?;

    let mut items: Vec<CatalogItem> = parsed
        .response
        .items
        .iter()
        .map(|item| CatalogItem {
            id: item.id,
            owner_id: item.owner_id,
            title: item.title.clone(),
            duration: item.duration_hms(),
            live_status: item.live_status.clone(),
            date: item.date,
            thumb: item.thumb_url().unwrap_or("").to_string(),
        })
        .collect();
    items.sort_by(|a, b| b.id.cmp(&a.id));

    match parsed.response.livestream() {
        Some(live) => {
            tracing::info!(
                "Live detectado oid={} id={} title={}",
                live.owner_id,
                live.id,
                live.title
            );
            Ok(PrefetchResult {
                found: true,
                vk_oid: live.owner_id.to_string(),
                vk_id: live.id.to_string(),
                items,
            })
        }
        None => {
            tracing::info!("Sin live (live_status=started); items={}", items.len());
            Ok(PrefetchResult {
                found: false,
                vk_oid: NOT_FOUND_OID.into(),
                vk_id: NOT_FOUND_VID.into(),
                items,
            })
        }
    }
}

pub fn catalog_from_bytes(bytes: &[u8]) -> Result<PrefetchResult> {
    let (value, _) = extract_prefetch_value(bytes)?;
    catalog_from_prefetch(&value)
}

/// Fetch profile HTML and build live + VOD catalog (Chrome UA).
pub async fn prefetch(client: &reqwest::Client, idvk: &str) -> Result<PrefetchResult> {
    let (_url, bytes) = fetch_profile_bytes(client, idvk, false).await?;
    catalog_from_bytes(&bytes)
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn skip_ws(b: &[u8]) -> &[u8] {
    match b.iter().position(|c| !c.is_ascii_whitespace()) {
        Some(i) => &b[i..],
        None => b,
    }
}
