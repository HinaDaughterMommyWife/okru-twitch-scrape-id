//! VK HTML scraper — 1:1 port of migrate/vk/vk_stream_check.py

use anyhow::{Context, Result};
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::OnceLock;

const USER_AGENT: &str =
    "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";
const DEFAULT_TIMEOUT_SECS: u64 = 30;

pub const NOT_FOUND_OID: &str = "NOT_FOUND";
pub const NOT_FOUND_VID: &str = "NOT_FOUND";

#[derive(Debug, Clone, Serialize)]
pub struct LiveHit {
    pub token: String,
    pub vk_oid: String,
    pub vk_id: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeResult {
    pub url: String,
    pub active_live: bool,
    pub live: Option<LiveHit>,
    pub newest_vod: Option<LiveHit>,
    pub profile_videos: Vec<LiveHit>,
    pub data_video_count: usize,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_live_candidates: Option<Vec<LiveHit>>,
}

fn re_data_video() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"data-video="(-?\d+_\d+)""#).unwrap())
}

fn re_video_token() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"video(-?\d+)_(\d+)").unwrap())
}

fn re_live_block() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?s)data-video="(-?\d+_\d+)"[^>]*data-duration="0""#).unwrap()
    })
}

fn re_live_class() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?s)class="video_thumb_label _live"[\s\S]{0,800}?data-video="(-?\d+_\d+)"|data-video="(-?\d+_\d+)"[\s\S]{0,800}?class="video_thumb_label _live""#,
        )
        .unwrap()
    })
}

fn re_video_block() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?s)<div class="video-block-web">([\s\S]*?)</div>\s*</div>"#).unwrap()
    })
}

fn re_href_video() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"href="/video(-?\d+)_(\d+)""#).unwrap())
}

fn re_duration_label() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"<span class="duration-label">([^<]+)</span>"#).unwrap())
}

fn re_has_digit() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d").unwrap())
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

fn split_video_token(token: &str) -> (String, String) {
    let (oid, vid) = token.split_once('_').unwrap_or((token, ""));
    (oid.to_string(), vid.to_string())
}

fn vk_page_headers(page_url: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, REFERER, USER_AGENT as UA};
    let mut h = HeaderMap::new();
    h.insert(UA, HeaderValue::from_static(USER_AGENT));
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
    h.insert(
        "Sec-Fetch-Dest",
        HeaderValue::from_static("document"),
    );
    h.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
    let site = if page_url.contains("vk.com") {
        "same-origin"
    } else {
        "none"
    };
    h.insert("Sec-Fetch-Site", HeaderValue::from_static(site));
    h.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
    h.insert(
        "From",
        HeaderValue::from_static("googlebot(at)googlebot.com"),
    );
    h.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));
    h.insert("Pragma", HeaderValue::from_static("no-cache"));
    h
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<String> {
    tracing::debug!("GET {url}");
    let resp = client
        .get(url)
        .headers(vk_page_headers(url))
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error for {url}"))?;
    let bytes = resp.bytes().await.context("leer body VK")?;
    let html = String::from_utf8_lossy(&bytes).into_owned();
    tracing::info!("VK fetch OK url={url} bytes={}", html.len());
    Ok(html)
}

fn find_wall_live(html: &str) -> Vec<LiveHit> {
    let mut found = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for re in [re_live_block(), re_live_class()] {
        for caps in re.captures_iter(html) {
            let token = caps
                .iter()
                .skip(1)
                .flatten()
                .map(|m| m.as_str())
                .next()
                .unwrap_or("");
            if token.is_empty() || seen.contains(token) {
                continue;
            }
            seen.insert(token.to_string());
            let (oid, vid) = split_video_token(token);
            found.push(LiveHit {
                token: token.to_string(),
                vk_oid: oid,
                vk_id: vid,
                source: "wall_live".into(),
                duration: None,
            });
        }
    }
    found
}

fn find_profile_vods(html: &str) -> Vec<LiveHit> {
    let mut vods = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for caps in re_video_block().captures_iter(html) {
        let block = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let Some(href) = re_href_video().captures(block) else {
            continue;
        };
        let oid = href.get(1).unwrap().as_str();
        let vid = href.get(2).unwrap().as_str();
        let token = format!("{oid}_{vid}");
        if seen.contains(&token) {
            continue;
        }
        seen.insert(token.clone());
        let duration = re_duration_label()
            .captures(block)
            .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()));
        vods.push(LiveHit {
            token,
            vk_oid: oid.to_string(),
            vk_id: vid.to_string(),
            source: "profile_videos".into(),
            duration,
        });
    }

    if !vods.is_empty() {
        return vods;
    }

    for caps in re_video_token().captures_iter(html) {
        let oid = caps.get(1).unwrap().as_str();
        let vid = caps.get(2).unwrap().as_str();
        let token = format!("{oid}_{vid}");
        if seen.contains(&token) {
            continue;
        }
        seen.insert(token.clone());
        vods.push(LiveHit {
            token,
            vk_oid: oid.to_string(),
            vk_id: vid.to_string(),
            source: "video_link".into(),
            duration: None,
        });
    }
    vods
}

fn newest_by_vk_id(entries: &[LiveHit]) -> Option<LiveHit> {
    entries
        .iter()
        .max_by_key(|e| e.vk_id.parse::<u64>().unwrap_or(0))
        .cloned()
}

fn is_profile_live_duration(duration: Option<&str>) -> bool {
    let Some(duration) = duration else {
        return false;
    };
    let label = duration.trim().to_lowercase();
    if label == "live" || label == "en vivo" {
        return true;
    }
    !label.is_empty() && !re_has_digit().is_match(&label) && label.contains("live")
}

fn find_profile_live(profile_videos: &[LiveHit]) -> Vec<LiveHit> {
    profile_videos
        .iter()
        .filter(|v| is_profile_live_duration(v.duration.as_deref()))
        .map(|v| LiveHit {
            source: "profile_live".into(),
            ..v.clone()
        })
        .collect()
}

pub async fn analyze(client: &reqwest::Client, url: &str) -> Result<AnalyzeResult> {
    let page_url = normalize_url(url);
    tracing::info!("Analizando {page_url}");
    let html = fetch(client, &page_url).await?;

    let live_posts = find_wall_live(&html);
    let profile_vods = find_profile_vods(&html);
    let all_data_videos: Vec<_> = re_data_video()
        .captures_iter(&html)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();

    let mut result = AnalyzeResult {
        url: page_url,
        active_live: false,
        live: None,
        newest_vod: None,
        profile_videos: profile_vods.clone(),
        data_video_count: all_data_videos.len(),
        note: String::new(),
        all_live_candidates: None,
    };

    if !live_posts.is_empty() {
        let live = live_posts[0].clone();
        tracing::info!("Live detectado token={}", live.token);
        result.active_live = true;
        result.live = Some(live);
        result.all_live_candidates = Some(live_posts);
        result.note =
            "Directo activo detectado en el muro (_live + data-duration=0).".into();
        return Ok(result);
    }

    if !profile_vods.is_empty() {
        let profile_live = find_profile_live(&profile_vods);
        if !profile_live.is_empty() {
            let live = newest_by_vk_id(&profile_live).unwrap();
            tracing::info!(
                "Live en perfil token={} duration={:?}",
                live.token,
                live.duration
            );
            let non_live: Vec<_> = profile_vods
                .iter()
                .filter(|v| !is_profile_live_duration(v.duration.as_deref()))
                .cloned()
                .collect();
            result.active_live = true;
            result.live = Some(live);
            result.all_live_candidates = Some(profile_live);
            result.newest_vod = newest_by_vk_id(&non_live);
            result.note =
                "Directo en perfil (duration-label Live, sin tiempo M:SS).".into();
            return Ok(result);
        }

        result.newest_vod = newest_by_vk_id(&profile_vods);
        result.note =
            "Solo grabaciones en el perfil (duration-label). No hay directo activo en el HTML."
                .into();
        tracing::info!("Sin live; VODs en perfil={}", profile_vods.len());
        return Ok(result);
    }

    if !all_data_videos.is_empty() {
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        for token in &all_data_videos {
            if !seen.insert(token.clone()) {
                continue;
            }
            let (oid, vid) = split_video_token(token);
            entries.push(LiveHit {
                token: token.clone(),
                vk_oid: oid,
                vk_id: vid,
                source: "data_video".into(),
                duration: None,
            });
        }
        result.newest_vod = newest_by_vk_id(&entries);
        result.note =
            "Hay data-video pero sin marca _live en el HTML. Tratar como sin directo activo (posibles replays)."
                .into();
        return Ok(result);
    }

    result.note = "No se encontraron vídeos en la página.".into();
    Ok(result)
}

/// Run scrape + decide oid/vid for worker POST.
pub async fn check_and_ids(
    client: &reqwest::Client,
    idvk: &str,
) -> Result<(bool, String, String)> {
    let result = analyze(client, idvk).await?;
    if result.active_live {
        if let Some(live) = result.live {
            return Ok((true, live.vk_oid, live.vk_id));
        }
    }
    Ok((false, NOT_FOUND_OID.into(), NOT_FOUND_VID.into()))
}
