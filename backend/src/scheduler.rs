//! Activity window for the periodic VK check loop.
//!
//! Default: OFF.
//! On `#vk`: activate / refresh an 8-hour window from the last command.
//! After 8h without `#vk`: the loop turns OFF again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};

const WINDOW: Duration = Duration::from_secs(8 * 60 * 60);

#[derive(Debug)]
pub struct Scheduler {
    /// Last time a privileged `#vk` was received (None = never / window expired).
    last_command: RwLock<Option<Instant>>,
    /// Wakes the loop when `#vk` arrives while sleeping.
    notify: Notify,
    /// Soft stop flag for clean shutdown.
    running: AtomicBool,
}

impl Scheduler {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last_command: RwLock::new(None),
            notify: Notify::new(),
            running: AtomicBool::new(true),
        })
    }

    /// Called when `#vk` fires — (re)starts the 8h window.
    pub async fn bump(&self) {
        let mut slot = self.last_command.write().await;
        *slot = Some(Instant::now());
        drop(slot);
        self.notify.notify_one();
        tracing::info!("Intervalo activado / reiniciado (ventana 8h)");
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.notify.notify_one();
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// True while inside the 8h activity window.
    pub async fn is_active(&self) -> bool {
        let slot = self.last_command.read().await;
        match *slot {
            Some(t) => t.elapsed() < WINDOW,
            None => false,
        }
    }

    /// Remaining time in the window, if active.
    #[allow(dead_code)]
    pub async fn remaining(&self) -> Option<Duration> {
        let slot = self.last_command.read().await;
        match *slot {
            Some(t) => {
                let elapsed = t.elapsed();
                if elapsed < WINDOW {
                    Some(WINDOW - elapsed)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Wait until the window becomes active (a `#vk` arrives), or shutdown.
    pub async fn wait_until_active(&self) {
        loop {
            if !self.is_running() {
                return;
            }
            if self.is_active().await {
                return;
            }
            // Expire stale timestamp if any
            {
                let mut slot = self.last_command.write().await;
                if let Some(t) = *slot {
                    if t.elapsed() >= WINDOW {
                        *slot = None;
                        tracing::info!("Ventana de 8h expirada — intervalo apagado");
                    }
                }
            }
            self.notify.notified().await;
        }
    }

    /// Sleep for `interval` (does not wake on `#vk` — avoids double-scrape with the command handler).
    pub async fn sleep_interval(&self, interval: Duration) {
        tokio::time::sleep(interval).await;
    }
}
