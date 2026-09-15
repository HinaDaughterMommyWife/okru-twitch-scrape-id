use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use worker::*;

const KV_BINDING: &str = "OKRU_ID";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserProfile {
    slug: String,
    display_name: String,
    twitch_channel: String,
    idvk: String,
    command: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
struct StreamPostBody {
    vk_oid: String,
    vk_id: String,
    /// When present, the profile is upserted too (a chat command instantiates the page).
    user: Option<UserProfile>,
}

#[derive(Serialize, Deserialize)]
struct KvEntry {
    vk_oid: String,
    vk_id: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VodItem {
    id: i64,
    owner_id: i64,
    title: String,
    duration: String,
    live_status: String,
    date: i64,
    thumb: String,
}

#[derive(Deserialize)]
struct VodsPostBody {
    items: Vec<VodItem>,
}

#[derive(Serialize, Deserialize)]
struct VodsEntry {
    items: Vec<VodItem>,
    updated_at: String,
}

#[derive(Serialize)]
struct UserResponse {
    user: UserProfile,
    streaming: Option<KvEntry>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn user_key(slug: &str) -> String {
    format!("user:{slug}")
}

fn stream_key(slug: &str) -> String {
    format!("stream:{slug}")
}

fn vods_key(slug: &str) -> String {
    format!("vods:{slug}")
}

fn is_authorized(req: &Request, ctx: &RouteContext<()>) -> bool {
    let secret = match ctx.secret("AUTH_TOKEN") {
        Ok(s) => s.to_string(),
        Err(_) => return false,
    };

    let header = match req.headers().get("Authorization").ok().flatten() {
        Some(h) => h,
        None => return false,
    };

    let encoded = match header.strip_prefix("Basic ") {
        Some(e) => e,
        None => return false,
    };

    let decoded = match STANDARD.decode(encoded) {
        Ok(d) => String::from_utf8_lossy(&d).to_string(),
        Err(_) => return false,
    };

    decoded == format!("admin:{}", secret)
}

fn unauthorized() -> Result<Response> {
    let mut resp = json_response(
        401,
        &ErrorResponse {
            error: "unauthorized".into(),
        },
    )?;
    resp.headers_mut()
        .set("WWW-Authenticate", "Basic realm=\"okru-worker\"")?;
    Ok(resp)
}

fn json_response(status: u16, body: &impl Serialize) -> Result<Response> {
    let json = serde_json::to_string(body).unwrap();
    let mut resp = Response::ok(json)?.with_status(status);
    resp.headers_mut().set("Content-Type", "application/json")?;
    resp.headers_mut().set("Cache-Control", "no-store")?;
    Ok(resp)
}

fn error(status: u16, msg: &str) -> Result<Response> {
    json_response(status, &ErrorResponse { error: msg.into() })
}

fn now_iso() -> String {
    Date::now().to_string()
}

/// Same rules as okru-db slugs (lowercase a-z 0-9 _ -, max 32).
fn valid_slug(slug: &str) -> bool {
    (1..=32).contains(&slug.len())
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Auth + slug validation shared by every route. `Err(response)` short-circuits.
fn guard(req: &Request, ctx: &RouteContext<()>) -> std::result::Result<String, Result<Response>> {
    if !is_authorized(req, ctx) {
        return Err(unauthorized());
    }
    match ctx.param("slug") {
        Some(slug) if valid_slug(slug) => Ok(slug.to_string()),
        _ => Err(error(400, "invalid slug")),
    }
}

async fn kv_get_json<T: DeserializeOwned>(kv: &kv::KvStore, key: &str) -> Result<Option<T>> {
    Ok(kv
        .get(key)
        .text()
        .await?
        .and_then(|raw| serde_json::from_str::<T>(&raw).ok()))
}

async fn kv_put_json(kv: &kv::KvStore, key: &str, value: &impl Serialize) -> Result<()> {
    kv.put(key, serde_json::to_string(value).unwrap())?
        .execute()
        .await?;
    Ok(())
}

async fn put_profile(kv: &kv::KvStore, slug: &str, mut profile: UserProfile) -> Result<UserProfile> {
    profile.slug = slug.to_string();
    profile.updated_at = now_iso();
    kv_put_json(kv, &user_key(slug), &profile).await?;
    Ok(profile)
}

/// Slugs of every stored profile — lets the backend delete users removed from its DB.
async fn handle_users_list(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !is_authorized(&req, &ctx) {
        return unauthorized();
    }
    let kv = ctx.kv(KV_BINDING)?;
    let mut slugs = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut list = kv.list().prefix("user:".into());
        if let Some(c) = cursor.take() {
            list = list.cursor(c);
        }
        let page = list.execute().await?;
        slugs.extend(
            page.keys
                .into_iter()
                .filter_map(|k| k.name.strip_prefix("user:").map(String::from)),
        );
        match page.cursor {
            Some(c) if !page.list_complete => cursor = Some(c),
            _ => break,
        }
    }
    json_response(200, &serde_json::json!({ "slugs": slugs }))
}

async fn handle_user_get(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match guard(&req, &ctx) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let kv = ctx.kv(KV_BINDING)?;
    let Some(user) = kv_get_json::<UserProfile>(&kv, &user_key(&slug)).await? else {
        return error(404, "user not found");
    };
    let streaming = kv_get_json::<KvEntry>(&kv, &stream_key(&slug)).await?;
    json_response(200, &UserResponse { user, streaming })
}

async fn handle_user_put(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match guard(&req, &ctx) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let profile: UserProfile = match req.json().await {
        Ok(p) => p,
        Err(_) => return error(400, "invalid JSON body – expected user profile"),
    };
    let kv = ctx.kv(KV_BINDING)?;
    let saved = put_profile(&kv, &slug, profile).await?;
    json_response(200, &saved)
}

async fn handle_user_delete(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match guard(&req, &ctx) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let kv = ctx.kv(KV_BINDING)?;
    for key in [user_key(&slug), stream_key(&slug), vods_key(&slug)] {
        kv.delete(&key).await?;
    }
    json_response(200, &serde_json::json!({ "deleted": slug }))
}

async fn handle_stream_post(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match guard(&req, &ctx) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let body: StreamPostBody = match req.json().await {
        Ok(b) => b,
        Err(_) => {
            return error(
                400,
                "invalid JSON body – expected {\"vk_oid\": \"...\", \"vk_id\": \"...\"}",
            )
        }
    };

    let kv = ctx.kv(KV_BINDING)?;
    if let Some(profile) = body.user {
        put_profile(&kv, &slug, profile).await?;
    } else if kv.get(&user_key(&slug)).text().await?.is_none() {
        return error(404, "user not found");
    }

    let entry = KvEntry {
        vk_oid: body.vk_oid,
        vk_id: body.vk_id,
        updated_at: now_iso(),
    };
    kv_put_json(&kv, &stream_key(&slug), &entry).await?;
    json_response(200, &entry)
}

async fn handle_stream_get(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match guard(&req, &ctx) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let kv = ctx.kv(KV_BINDING)?;
    match kv_get_json::<KvEntry>(&kv, &stream_key(&slug)).await? {
        Some(entry) => json_response(200, &entry),
        None => error(404, "no streaming id stored yet"),
    }
}

async fn handle_vods_post(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match guard(&req, &ctx) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut body: VodsPostBody = match req.json().await {
        Ok(b) => b,
        Err(_) => return error(400, "invalid JSON body – expected {\"items\": [...]}"),
    };

    body.items.sort_by(|a, b| b.id.cmp(&a.id));

    let kv = ctx.kv(KV_BINDING)?;
    let entry = VodsEntry {
        items: body.items,
        updated_at: now_iso(),
    };
    kv_put_json(&kv, &vods_key(&slug), &entry).await?;
    json_response(200, &entry)
}

async fn handle_vods_get(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match guard(&req, &ctx) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let kv = ctx.kv(KV_BINDING)?;
    match kv_get_json::<VodsEntry>(&kv, &vods_key(&slug)).await? {
        Some(entry) => json_response(200, &entry),
        None => error(404, "no vods stored yet"),
    }
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/users", handle_users_list)
        .get_async("/users/:slug", handle_user_get)
        .put_async("/users/:slug", handle_user_put)
        .delete_async("/users/:slug", handle_user_delete)
        .post_async("/users/:slug/streaming", handle_stream_post)
        .get_async("/users/:slug/streaming", handle_stream_get)
        .post_async("/users/:slug/vods", handle_vods_post)
        .get_async("/users/:slug/vods", handle_vods_get)
        .run(req, env)
        .await
}
