use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use worker::*;

const KV_KEY: &str = "current";

#[derive(Deserialize)]
struct PostBody {
    streaming_id: String,
    #[allow(dead_code)]
    source_url: Option<String>,
    #[allow(dead_code)]
    timestamp: Option<String>,
}

#[derive(Serialize)]
struct KvEntry {
    streaming_id: String,
    updated_at: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
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
    let body = serde_json::to_string(&ErrorResponse {
        error: "unauthorized".into(),
    })
    .unwrap();
    let mut resp = Response::ok(body)?;
    resp = resp.with_status(401);
    resp.headers_mut()
        .set("WWW-Authenticate", "Basic realm=\"okru-worker\"")?;
    resp.headers_mut().set("Content-Type", "application/json")?;
    Ok(resp)
}

fn json_response(status: u16, body: &impl Serialize) -> Result<Response> {
    let json = serde_json::to_string(body).unwrap();
    let mut resp = Response::ok(json)?;
    resp = resp.with_status(status);
    resp.headers_mut().set("Content-Type", "application/json")?;
    Ok(resp)
}

fn now_iso() -> String {
    Date::now().to_string()
}

async fn handle_post(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !is_authorized(&req, &ctx) {
        return unauthorized();
    }

    let body: PostBody = match req.json().await {
        Ok(b) => b,
        Err(_) => {
            return json_response(
                400,
                &ErrorResponse {
                    error: "invalid JSON body – expected {\"streaming_id\": \"...\"}".into(),
                },
            )
        }
    };

    let kv = ctx.kv("OKRU_ID")?;

    let entry = KvEntry {
        streaming_id: body.streaming_id,
        updated_at: now_iso(),
    };

    kv.put(KV_KEY, serde_json::to_string(&entry).unwrap())?
        .execute()
        .await?;

    json_response(200, &entry)
}

async fn handle_get(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !is_authorized(&req, &ctx) {
        return unauthorized();
    }

    let kv = ctx.kv("OKRU_ID")?;

    match kv.get(KV_KEY).text().await? {
        Some(value) => {
            let mut resp = Response::ok(value)?;
            resp.headers_mut().set("Content-Type", "application/json")?;
            resp.headers_mut().set("Cache-Control", "no-store")?;
            Ok(resp)
        }
        None => json_response(
            404,
            &ErrorResponse {
                error: "no streaming id stored yet".into(),
            },
        ),
    }
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .post_async("/streaming", handle_post)
        .get_async("/streaming", handle_get)
        .run(req, env)
        .await
}
