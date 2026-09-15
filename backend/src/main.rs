mod bot;
mod check;
mod config;
mod credentials;
mod http;
mod ipc;
mod registry;
mod scheduler;
mod sync;
mod vk;
mod worker_client;

use anyhow::{Context, Result};
use okru_tui::db::Store;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Notify};
use tracing_subscriber::EnvFilter;

use crate::credentials::credentials_exist;
use crate::registry::Snapshot;
use crate::scheduler::Scheduler;
use crate::worker_client::WorkerClient;

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
        "Config OK — intervalo={}min bot={} worker={} web={}",
        cfg.intervalo,
        cfg.bot_login(),
        cfg.worker_base(),
        cfg.web_url
    );
    tracing::info!(
        "OAuth setup: {}/{}/setup",
        cfg.base_url.trim_end_matches('/'),
        cfg.setup_path_key
    );

    // SQLite users → snapshot, hot-reloaded by a watcher thread.
    let db_path = okru_tui::db::default_db_path();
    let store =
        Store::open(&db_path).with_context(|| format!("abrir DB {}", db_path.display()))?;
    let snapshot = Snapshot::from_users(store.list()?);
    tracing::info!(
        "DB {} — {} usuarios ({} activos)",
        db_path.display(),
        snapshot.total,
        snapshot.len()
    );
    if snapshot.len() == 0 {
        tracing::warn!("Sin usuarios activos — agrega uno con okru-tui");
    }
    let (snapshot_tx, snapshot_rx) = watch::channel(Arc::new(snapshot));

    // okru-tui ↔ backend: `changed` → immediate reload; events flow back to the TUI.
    let hub = ipc::Hub::new();
    let (reload_tx, reload_rx) = mpsc::unbounded_channel();
    tokio::spawn(registry::run_reloader(store, reload_rx, snapshot_tx, Arc::clone(&hub)));
    {
        let (hub, rx) = (Arc::clone(&hub), snapshot_rx.clone());
        let port = cfg.ipc_port;
        tokio::spawn(async move {
            if let Err(e) = ipc::serve(port, hub, reload_tx, rx).await {
                tracing::error!("IPC server error — los cambios del TUI no se aplicarán en caliente: {e:#}");
            }
        });
    }

    let http = reqwest::Client::builder()
        .user_agent("okru-backend/0.1")
        .gzip(true)
        .build()?;
    let worker = WorkerClient::new(http.clone(), cfg.worker_base(), &cfg.post_auth);

    tokio::spawn(sync::run(worker.clone(), snapshot_rx.clone(), Arc::clone(&hub)));

    let credentials_ready = Arc::new(Notify::new());
    let scheduler = Scheduler::new(
        http.clone(),
        worker.clone(),
        snapshot_rx.clone(),
        Duration::from_secs(cfg.intervalo.max(1) * 60),
    );

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
        match bot::run_bot(
            Arc::clone(&cfg),
            http.clone(),
            worker.clone(),
            Arc::clone(&scheduler),
            snapshot_rx.clone(),
            Arc::clone(&hub),
        )
        .await
        {
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
