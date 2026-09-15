//! Per-user activity windows for the periodic VK check loop.
//!
//! Default: OFF for everyone.
//! On a chat command: activate / refresh an 8-hour window for that user and start its loop.
//! After 8h without commands (or if the user is removed/disabled) the loop ends.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::check::run_check;
use crate::registry::SnapshotRx;
use crate::worker_client::WorkerClient;

const WINDOW: Duration = Duration::from_secs(8 * 60 * 60);

pub struct Scheduler {
    /// user id → last command. Presence means that user's loop is running.
    windows: Mutex<HashMap<i64, Instant>>,
    http: reqwest::Client,
    worker: WorkerClient,
    rx: SnapshotRx,
    interval: Duration,
}

impl Scheduler {
    pub fn new(http: reqwest::Client, worker: WorkerClient, rx: SnapshotRx, interval: Duration) -> Arc<Self> {
        tracing::info!(
            "Interval scheduler ready (default OFF, period={}min, window=8h, por usuario)",
            interval.as_secs() / 60
        );
        Arc::new(Self {
            windows: Mutex::new(HashMap::new()),
            http,
            worker,
            rx,
            interval,
        })
    }

    /// Called when a command fires — (re)starts the 8h window for this user.
    pub fn bump(self: &Arc<Self>, user_id: i64, slug: &str) {
        let started = self
            .windows
            .lock()
            .unwrap()
            .insert(user_id, Instant::now())
            .is_none();
        tracing::info!("[{slug}] Intervalo activado / reiniciado (ventana 8h)");
        if started {
            let this = Arc::clone(self);
            tokio::spawn(async move { this.interval_loop(user_id).await });
        }
    }

    /// Sleeps first so the command's immediate check is not duplicated.
    async fn interval_loop(self: Arc<Self>, user_id: i64) {
        loop {
            tokio::time::sleep(self.interval).await;

            let entry = self.rx.borrow().by_id(user_id);
            {
                let mut windows = self.windows.lock().unwrap();
                let active = windows.get(&user_id).is_some_and(|t| t.elapsed() < WINDOW);
                if !active || entry.is_none() {
                    windows.remove(&user_id);
                    let reason = if entry.is_none() { "usuario eliminado/desactivado" } else { "ventana de 8h expirada" };
                    tracing::info!("Intervalo apagado para user_id={user_id} ({reason})");
                    return;
                }
            }
            let Some(entry) = entry else { return };

            tracing::info!("[{}] --- interval tick ---", entry.user.slug);
            if let Err(e) = run_check(&self.http, &self.worker, &entry).await {
                tracing::error!("[{}] interval check failed: {e:#}", entry.user.slug);
            }
        }
    }
}
