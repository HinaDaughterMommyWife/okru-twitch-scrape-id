mod bot;
mod config;
mod credentials;
mod http;
mod scheduler;
mod vk;
mod worker_client;

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::credentials::credentials_exist;
use crate::scheduler::Scheduler;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = match config::load_or_create_template() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            // Template-created or validation failure: print and exit cleanly.
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    };

    tracing::info!(
        "Config OK — idvk={} intervalo={}min channel=#{} bot={}",
        cfg.idvk,
        cfg.intervalo,
        cfg.channel_login(),
        cfg.bot_login()
    );
    tracing::info!(
        "OAuth setup: {}/{}/setup",
        cfg.base_url.trim_end_matches('/'),
        cfg.setup_path_key
    );

    let http = reqwest::Client::builder()
        .user_agent("okru-backend/0.1")
        .gzip(true)
        .build()?;

    let credentials_ready = Arc::new(Notify::new());
    let scheduler = Scheduler::new();

    let http_state = http::AppState {
        config: Arc::clone(&cfg),
        oauth_state: Arc::new(tokio::sync::Mutex::new(None)),
        credentials_ready: Arc::clone(&credentials_ready),
        http: http.clone(),
    };

    // HTTP server (health + OAuth)
    tokio::spawn(async move {
        if let Err(e) = http::serve(http_state).await {
            tracing::error!("HTTP server error: {e:#}");
        }
    });

    // Periodic VK loop — OFF by default; `#vk` opens an 8h window.
    {
        let cfg = Arc::clone(&cfg);
        let http = http.clone();
        let scheduler = Arc::clone(&scheduler);
        tokio::spawn(async move {
            interval_loop(cfg, http, scheduler).await;
        });
    }

    // Bot lifecycle: wait for credentials, run, restart on drop/OAuth clear.
    loop {
        if !credentials_exist() {
            tracing::info!(
                "No credentials — visit {}/{}/setup",
                cfg.base_url.trim_end_matches('/'),
                cfg.setup_path_key
            );
            credentials_ready.notified().await;
            // After clear, loop back; after oauth, credentials exist.
            if !credentials_exist() {
                continue;
            }
        }

        tracing::info!("Starting Twitch bot...");
        match bot::run_bot(Arc::clone(&cfg), http.clone(), Arc::clone(&scheduler)).await {
            Ok(()) => tracing::warn!("Bot IRC stream ended"),
            Err(e) => tracing::error!("Bot error: {e:#}"),
        }

        // Brief pause then wait for credentials again (may have been cleared).
        tokio::time::sleep(Duration::from_secs(2)).await;
        if !credentials_exist() {
            credentials_ready.notified().await;
        } else {
            // Unexpected disconnect — reconnect soon.
            tracing::info!("Reconnecting bot in 5s...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn interval_loop(cfg: Arc<Config>, http: reqwest::Client, scheduler: Arc<Scheduler>) {
    let interval = Duration::from_secs(cfg.intervalo.max(1) * 60);
    tracing::info!(
        "Interval scheduler ready (default OFF, period={}min, window=8h)",
        cfg.intervalo
    );

    loop {
        if !scheduler.is_running() {
            break;
        }

        scheduler.wait_until_active().await;
        if !scheduler.is_running() {
            break;
        }

        // Sleep first so `#vk`'s immediate check is not duplicated by this loop.
        scheduler.sleep_interval(interval).await;

        if !scheduler.is_running() {
            break;
        }
        if !scheduler.is_active().await {
            tracing::info!("Ventana de 8h expirada — intervalo apagado");
            continue;
        }

        tracing::info!("--- interval tick ---");
        match vk::check_and_ids(&http, &cfg.idvk).await {
            Ok((found, oid, vid)) => {
                tracing::info!("interval scrape found={found} oid={oid} vid={vid}");
                if let Err(e) =
                    worker_client::post_to_worker(&http, &cfg.post_url, &cfg.post_auth, &oid, &vid)
                        .await
                {
                    tracing::error!("interval POST failed: {e:#}");
                }
            }
            Err(e) => tracing::error!("interval scrape failed: {e:#}"),
        }
    }
}
