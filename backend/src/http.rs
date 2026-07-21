//! Minimal HTTP server: /health + Twitch OAuth setup routes.

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

use crate::config::Config;
use crate::credentials::{
    clear_credentials, credentials_exist, save_credentials, StoredCredentials,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub oauth_state: Arc<Mutex<Option<String>>>,
    /// Fired when OAuth completes successfully so main can (re)start the bot.
    pub credentials_ready: Arc<Notify>,
    pub http: reqwest::Client,
}

pub fn router(state: AppState) -> Router {
    let key = state.config.setup_path_key.clone();
    Router::new()
        .route("/health", get(health))
        .route(&format!("/{key}/setup"), get(setup_page))
        .route(&format!("/{key}/oauth"), get(oauth_redirect))
        .route(&format!("/{key}/callback"), get(oauth_callback))
        .route(&format!("/{key}/clear"), get(clear_endpoint))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "bot_active": credentials_exist(),
    }))
}

async fn setup_page(State(state): State<AppState>) -> Response {
    if credentials_exist() {
        return (
            axum::http::StatusCode::CONFLICT,
            "Bot credentials already configured",
        )
            .into_response();
    }
    let key = &state.config.setup_path_key;
    let html = format!(
        r#"<!DOCTYPE html><html><head><title>VK Bot Setup</title></head>
<body style="font-family:monospace;max-width:600px;margin:4rem auto;padding:1rem">
<h2>VK Twitch Bot — Setup</h2>
<p>Credentials not yet configured.</p>
<a href="/{key}/oauth" style="display:inline-block;padding:.6rem 1.2rem;background:#9147ff;color:#fff;text-decoration:none;border-radius:4px">
  Autorizar con Twitch
</a>
</body></html>"#
    );
    Html(html).into_response()
}

async fn oauth_redirect(State(state): State<AppState>) -> Response {
    if credentials_exist() {
        return (
            axum::http::StatusCode::CONFLICT,
            "Already configured",
        )
            .into_response();
    }

    let nonce = format!("{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    *state.oauth_state.lock().await = Some(nonce.clone());

    let key = &state.config.setup_path_key;
    let redirect_uri = format!("{}/{key}/callback", state.config.base_url.trim_end_matches('/'));
    let params = [
        ("client_id", state.config.twitch_token_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", "chat:read chat:edit"),
        ("state", nonce.as_str()),
        ("force_verify", "true"),
    ];
    let qs = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    Redirect::temporary(&format!(
        "https://id.twitch.tv/oauth2/authorize?{qs}"
    ))
    .into_response()
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn oauth_callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if let Some(err) = q.error {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            format!("OAuth error: {err}"),
        )
            .into_response();
    }

    let expected = state.oauth_state.lock().await.take();
    let Some(expected) = expected else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Missing OAuth state — restart flow",
        )
            .into_response();
    };
    if q.state.as_deref() != Some(expected.as_str()) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid OAuth state",
        )
            .into_response();
    }

    let Some(code) = q.code else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Missing code",
        )
            .into_response();
    };

    let key = &state.config.setup_path_key;
    let redirect_uri = format!("{}/{key}/callback", state.config.base_url.trim_end_matches('/'));

    let mut form = HashMap::new();
    form.insert("client_id", state.config.twitch_token_id.as_str());
    form.insert("client_secret", state.config.twitch_token_secret.as_str());
    form.insert("code", code.as_str());
    form.insert("grant_type", "authorization_code");
    form.insert("redirect_uri", redirect_uri.as_str());

    let resp = match state
        .http
        .post("https://id.twitch.tv/oauth2/token")
        .form(&form)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Token exchange failed: {e}"),
            )
                .into_response();
        }
    };

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("Twitch token exchange failed: {text}"),
        )
            .into_response();
    }

    let data: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Invalid token JSON: {e}"),
            )
                .into_response();
        }
    };

    match StoredCredentials::from_oauth_response(&data) {
        Ok(creds) => {
            if let Err(e) = save_credentials(&creds) {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to save credentials: {e}"),
                )
                    .into_response();
            }
        }
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Bad oauth payload: {e}"),
            )
                .into_response();
        }
    }

    state.credentials_ready.notify_waiters();

    Html(
        r#"<!DOCTYPE html><html><head><title>Setup Complete</title></head>
<body style="font-family:monospace;max-width:600px;margin:4rem auto;padding:1rem">
<h2>Bot autorizado y configurado</h2>
<p>El bot de Twitch está ahora activo. Puedes cerrar esta ventana.</p>
</body></html>"#,
    )
    .into_response()
}

async fn clear_endpoint(State(state): State<AppState>) -> Response {
    let _ = clear_credentials();
    state.credentials_ready.notify_waiters();
    let key = &state.config.setup_path_key;
    Html(format!(
        r#"<html><body style='font-family:monospace;max-width:600px;margin:4rem auto'>
<h2>Credenciales eliminadas</h2>
<p><a href='/{key}/setup'>Volver a configurar</a></p>
</body></html>"#
    ))
    .into_response()
}

pub async fn serve(state: AppState) -> anyhow::Result<()> {
    let port = state.config.port;
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("HTTP listening on 0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
