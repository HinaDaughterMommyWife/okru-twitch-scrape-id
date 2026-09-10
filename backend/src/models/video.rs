//! `video.get` entry inside `window.cur.apiPrefetchCache`.
//! Port of the TypeScript interfaces; extra VK keys are ignored.

use serde::{Deserialize, Serialize};
use serde_json::Value;

const THUMB_W: u32 = 1280;
const THUMB_H: u32 = 720;

/// VK marks the active livestream with this exact `live_status`.
pub const LIVE_STATUS_STARTED: &str = "started";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VideoGetResponse {
    pub method: String,
    pub request: VideoGetRequest,
    #[serde(default, deserialize_with = "flex_string")]
    pub version: String,
    pub response: VideoGetBody,
    #[serde(default)]
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VideoGetRequest {
    pub owner_id: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VideoGetBody {
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub items: Vec<VideoItem>,
    #[serde(default)]
    pub max_attached_short_videos: i64,
}

impl VideoGetBody {
    /// The livestream, if any: `live_status == "started"`.
    /// If more than one (rare), keep the highest video `id`.
    pub fn livestream(&self) -> Option<&VideoItem> {
        self.items
            .iter()
            .filter(|item| item.is_livestream())
            .max_by_key(|item| item.id)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VideoItem {
    #[serde(default)]
    pub files: Files,
    #[serde(default)]
    pub timeline_thumbs: Option<TimelineThumbs>,
    #[serde(default)]
    pub ads: Option<Ads>,
    #[serde(default)]
    pub can_be_pinned: bool,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub stats_pixels: Vec<StatsPixel>,
    #[serde(default)]
    pub need_mediascope_stat: bool,
    #[serde(default)]
    pub direct_url: String,
    #[serde(default)]
    pub share_url: String,
    #[serde(default)]
    pub response_type: String,
    #[serde(default)]
    pub adding_date: i64,
    #[serde(default)]
    pub can_like: i64,
    #[serde(default)]
    pub can_repost: i64,
    #[serde(default)]
    pub can_subscribe: i64,
    #[serde(default)]
    pub can_add: i64,
    #[serde(default)]
    pub can_play_in_background: i64,
    #[serde(default)]
    pub can_download: i64,
    #[serde(default)]
    pub download: Option<Download>,
    #[serde(default)]
    pub comments: i64,
    #[serde(default)]
    pub date: i64,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub image: Vec<Image>,
    #[serde(default)]
    pub first_frame: Vec<FirstFrame>,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    pub id: i64,
    pub owner_id: i64,
    #[serde(default)]
    pub ov_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub player: String,
    #[serde(default)]
    pub added: i64,
    #[serde(default)]
    pub track_code: String,
    #[serde(default)]
    pub tracking_info: Option<TrackingInfo>,
    #[serde(rename = "type", default)]
    pub item_type: String,
    #[serde(default)]
    pub views: i64,
    #[serde(default)]
    pub local_views: i64,
    #[serde(default)]
    pub live_status: String,
    #[serde(default)]
    pub likes: Option<Likes>,
    #[serde(default)]
    pub reposts: Option<Reposts>,
    #[serde(default)]
    pub can_dislike: i64,
    #[serde(default)]
    pub wall_post_id: i64,
    #[serde(default)]
    pub trailer: Option<Trailer>,
}

impl VideoItem {
    /// Active livestream. `postlive` / anything else is VOD or idle.
    pub fn is_livestream(&self) -> bool {
        self.live_status.eq_ignore_ascii_case(LIVE_STATUS_STARTED)
    }

    /// Seconds as `H:MM:SS` (e.g. 4387 → `1:13:07`).
    pub fn duration_hms(&self) -> String {
        format_hms(self.duration)
    }

    /// 1280×720 from `first_frame`, then `image`; otherwise the first URL that exists.
    pub fn thumb_url(&self) -> Option<&str> {
        pick_size(&self.first_frame, THUMB_W, THUMB_H)
            .or_else(|| pick_first(&self.first_frame))
            .or_else(|| pick_image_size(&self.image, THUMB_W, THUMB_H))
            .or_else(|| pick_image_first(&self.image))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Files {
    pub mp4_144: Option<String>,
    pub mp4_240: Option<String>,
    pub mp4_360: Option<String>,
    pub mp4_480: Option<String>,
    pub mp4_720: Option<String>,
    pub hls: Option<String>,
    pub dash_sep: Option<String>,
    pub hls_fmp4: Option<String>,
    pub failover_host: Option<String>,
    pub hls_ondemand: Option<String>,
    pub dash_ondemand: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TimelineThumbs {
    #[serde(default, deserialize_with = "flex_i64")]
    pub count_per_image: i64,
    #[serde(default, deserialize_with = "flex_i64")]
    pub count_per_row: i64,
    #[serde(default, deserialize_with = "flex_i64")]
    pub count_total: i64,
    #[serde(default, deserialize_with = "flex_i64")]
    pub frame_height: i64,
    #[serde(default, deserialize_with = "flex_i64")]
    pub frame_width: i64,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub is_uv: bool,
    #[serde(default, deserialize_with = "flex_i64")]
    pub frequency: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Ads {
    #[serde(default, deserialize_with = "flex_i64")]
    pub slot_id: i64,
    #[serde(default, deserialize_with = "flex_i64")]
    pub timeout: i64,
    #[serde(default, deserialize_with = "flex_i64")]
    pub can_play: i64,
    #[serde(default)]
    pub params: AdsParams,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub midroll_percents: Vec<f64>,
    #[serde(default, deserialize_with = "flex_i64")]
    pub autoplay_preroll: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AdsParams {
    #[serde(default, deserialize_with = "flex_string")]
    pub vk_id: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default, deserialize_with = "flex_string")]
    pub video_id: String,
    #[serde(default)]
    pub lang: i64,
    #[serde(default)]
    pub child_mode: bool,
    #[serde(default)]
    pub child_profile: bool,
    #[serde(default)]
    pub vk_catid: i64,
    #[serde(default)]
    pub is_xz_video: i64,
    #[serde(rename = "_SITEID", default)]
    pub site_id: i64,
    #[serde(default)]
    pub ad_nav_screen: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StatsPixel {
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub params: Option<StatsPixelParams>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StatsPixelParams {
    #[serde(default)]
    pub interval: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Download {
    #[serde(default)]
    pub can_download_for_offline_view: bool,
    #[serde(default)]
    pub can_download_to_device: bool,
    #[serde(default)]
    pub unavailable_for_offline_view: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Image {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub with_padding: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FirstFrame {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TrackingInfo {
    #[serde(default)]
    pub navigation: Navigation,
    #[serde(default)]
    pub recom_info: RecomInfo,
    #[serde(default)]
    pub search_info: SearchInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Navigation {
    #[serde(default)]
    pub source_screen: String,
    #[serde(default)]
    pub source_block: String,
    #[serde(default)]
    pub source_prev_screen: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RecomInfo {
    #[serde(default)]
    pub feature_sampling_uuid: String,
    #[serde(default)]
    pub recom_sources: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SearchInfo {
    #[serde(default)]
    pub search_query_id: String,
    #[serde(default)]
    pub search_iid: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Likes {
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub user_likes: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Reposts {
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub user_reposted: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Trailer {
    pub mp4_240: Option<String>,
    pub mp4_360: Option<String>,
    pub mp4_480: Option<String>,
    pub mp4_720: Option<String>,
}

fn format_hms(total_secs: i64) -> String {
    let secs = total_secs.max(0) as u64;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h}:{m:02}:{s:02}")
}

fn pick_size(frames: &[FirstFrame], w: u32, h: u32) -> Option<&str> {
    frames
        .iter()
        .find(|f| f.width == w && f.height == h)
        .map(|f| f.url.as_str())
}

fn pick_first(frames: &[FirstFrame]) -> Option<&str> {
    frames
        .first()
        .map(|f| f.url.as_str())
        .filter(|u| !u.is_empty())
}

fn pick_image_size(frames: &[Image], w: u32, h: u32) -> Option<&str> {
    frames
        .iter()
        .find(|f| f.width == w && f.height == h)
        .map(|f| f.url.as_str())
}

fn pick_image_first(frames: &[Image]) -> Option<&str> {
    frames
        .first()
        .map(|f| f.url.as_str())
        .filter(|u| !u.is_empty())
}

fn flex_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(0),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_u64().and_then(|u| i64::try_from(u).ok()))
            .or_else(|| n.as_f64().map(|f| f as i64))
            .ok_or_else(|| serde::de::Error::custom("not a number")),
        Value::Bool(b) => Ok(i64::from(b)),
        Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!(
            "expected number, got {other}"
        ))),
    }
}

fn flex_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {other}"
        ))),
    }
}
