//! Keeps worker profiles (`/users/:slug`) in sync with the SQLite snapshot.
//! - startup: publish every active user and delete worker profiles that are no longer in the DB
//! - on every DB change: upsert / delete only what changed
//! Failed calls stay pending and are retried periodically.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use okru_tui::ipc::ServerMsg;

use crate::ipc::Hub;
use crate::registry::{diff, Change, Snapshot, SnapshotRx};
use crate::worker_client::WorkerClient;

const RETRY: Duration = Duration::from_secs(30);

pub async fn run(worker: WorkerClient, mut rx: SnapshotRx, hub: Arc<Hub>) {
    let mut current: Arc<Snapshot> = rx.borrow_and_update().clone();
    let mut pending: HashMap<String, Change> = current
        .active()
        .map(|e| (e.user.slug.clone(), Change::Upsert(Arc::clone(e))))
        .collect();
    // Orphan cleanup needs the worker to be up (wrangler dev may still be compiling).
    let mut reconciled = false;
    sync_pass(&worker, &hub, &current, &mut pending, &mut reconciled).await;

    let mut retry = tokio::time::interval(RETRY);
    retry.tick().await;

    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let next = rx.borrow_and_update().clone();
                for change in diff(&current, &next) {
                    // Latest change per slug wins (e.g. remove then re-add).
                    pending.insert(change.slug().to_string(), change);
                }
                current = next;
                sync_pass(&worker, &hub, &current, &mut pending, &mut reconciled).await;
            }
            _ = retry.tick() => {
                if !pending.is_empty() || !reconciled {
                    sync_pass(&worker, &hub, &current, &mut pending, &mut reconciled).await;
                }
            }
        }
    }
}

async fn sync_pass(
    worker: &WorkerClient,
    hub: &Hub,
    current: &Snapshot,
    pending: &mut HashMap<String, Change>,
    reconciled: &mut bool,
) {
    if !*reconciled {
        match worker.list_users().await {
            Ok(slugs) => {
                for slug in orphans(current, &slugs) {
                    pending
                        .entry(slug.clone())
                        .or_insert(Change::Remove(slug));
                }
                *reconciled = true;
            }
            Err(e) => tracing::warn!(
                "Sync: no se pudo listar usuarios del worker (reintento en {}s): {e:#}",
                RETRY.as_secs()
            ),
        }
    }
    flush(worker, hub, pending).await;
}

/// Worker slugs with no active user in the DB.
fn orphans(current: &Snapshot, worker_slugs: &[String]) -> Vec<String> {
    worker_slugs
        .iter()
        .filter(|slug| !current.active().any(|e| &e.user.slug == *slug))
        .cloned()
        .collect()
}

async fn flush(worker: &WorkerClient, hub: &Hub, pending: &mut HashMap<String, Change>) {
    let changes: Vec<Change> = pending.values().cloned().collect();
    for change in changes {
        let result = match &change {
            Change::Upsert(entry) => worker.put_user(&entry.profile()).await,
            Change::Remove(slug) => worker.delete_user(slug).await,
        };
        let slug = change.slug().to_string();
        match result {
            Ok(()) => {
                pending.remove(&slug);
                let action = if matches!(change, Change::Upsert(_)) { "put" } else { "delete" };
                hub.emit(ServerMsg::Synced { slug, action: action.into() });
            }
            Err(e) => {
                tracing::warn!("Sync {slug} pendiente (reintento en {}s): {e:#}", RETRY.as_secs());
                hub.emit(ServerMsg::SyncFailed { slug, error: format!("{e:#}") });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use okru_tui::db::User;

    fn user(id: i64, slug: &str, enabled: bool) -> User {
        User {
            id,
            slug: slug.into(),
            display_name: slug.into(),
            twitch_channel: slug.into(),
            idvk: "id1".into(),
            enabled,
            commands: vec!["#vk".into()],
            whitelist: vec![],
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn orphans_are_worker_slugs_without_active_user() {
        let snap = Snapshot::from_users(vec![user(1, "perghor_adamia", true), user(2, "pausado", false)]);
        let worker = vec!["perghor_adamia".to_string(), "pausado".into(), "guibelorgulloperuano".into()];
        assert_eq!(orphans(&snap, &worker), vec!["pausado", "guibelorgulloperuano"]);
    }
}
