//! In-memory view of the users table, hot-reloaded from SQLite.
//!
//! okru-tui notifies every commit over the local IPC socket; the reloader re-reads the
//! table and publishes a fresh [`Snapshot`] through a `watch` channel.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use okru_tui::db::{clean_command, Store, User};
use okru_tui::ipc::ServerMsg;
use tokio::sync::{mpsc, watch};

use crate::ipc::Hub;
use crate::worker_client::PublicProfile;

pub type SnapshotRx = watch::Receiver<Arc<Snapshot>>;

#[derive(Debug)]
pub struct UserEntry {
    pub user: User,
    commands: HashSet<String>,
    whitelist: HashSet<String>,
}

impl UserEntry {
    fn new(user: User) -> Self {
        Self {
            commands: user.commands.iter().map(|c| clean_command(c)).collect(),
            whitelist: user.whitelist.iter().map(|l| l.to_lowercase()).collect(),
            user,
        }
    }

    /// True if the chat message is one of this user's aliases (ignores case/whitespace/invisible chars).
    pub fn matches_command(&self, raw: &str) -> bool {
        self.commands.contains(&clean_command(raw))
    }

    pub fn is_whitelisted(&self, login: &str) -> bool {
        self.whitelist.contains(&login.to_lowercase())
    }

    pub fn primary_command(&self) -> &str {
        self.user.commands.first().map(String::as_str).unwrap_or("#vk")
    }

    pub fn profile(&self) -> PublicProfile {
        PublicProfile {
            slug: self.user.slug.clone(),
            display_name: self.user.display_name.clone(),
            twitch_channel: self.user.twitch_channel.clone(),
            idvk: self.user.idvk.clone(),
            command: self.primary_command().to_string(),
        }
    }
}

/// Enabled users only — disabled users behave as if they did not exist.
#[derive(Debug, Default)]
pub struct Snapshot {
    by_id: BTreeMap<i64, Arc<UserEntry>>,
    by_channel: HashMap<String, Arc<UserEntry>>,
    pub total: usize,
}

impl Snapshot {
    pub fn from_users(users: Vec<User>) -> Self {
        let total = users.len();
        let mut snap = Snapshot { total, ..Default::default() };
        for user in users.into_iter().filter(|u| u.enabled) {
            let entry = Arc::new(UserEntry::new(user));
            snap.by_channel
                .insert(entry.user.twitch_channel.clone(), Arc::clone(&entry));
            snap.by_id.insert(entry.user.id, entry);
        }
        snap
    }

    pub fn by_channel(&self, channel: &str) -> Option<Arc<UserEntry>> {
        self.by_channel.get(channel).cloned()
    }

    pub fn by_id(&self, id: i64) -> Option<Arc<UserEntry>> {
        self.by_id.get(&id).cloned()
    }

    pub fn channels(&self) -> HashSet<String> {
        self.by_channel.keys().cloned().collect()
    }

    pub fn active(&self) -> impl Iterator<Item = &Arc<UserEntry>> {
        self.by_id.values()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }
}

#[derive(Debug, Clone)]
pub enum Change {
    Upsert(Arc<UserEntry>),
    Remove(String),
}

impl Change {
    pub fn slug(&self) -> &str {
        match self {
            Change::Upsert(e) => &e.user.slug,
            Change::Remove(slug) => slug,
        }
    }
}

/// What the worker needs to learn when going from `old` to `new`.
/// Only public-profile differences produce an upsert (whitelist edits are local).
pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Change> {
    let mut changes = Vec::new();
    for (id, before) in &old.by_id {
        match new.by_id.get(id) {
            Some(after) if after.user.slug == before.user.slug => {}
            _ => changes.push(Change::Remove(before.user.slug.clone())),
        }
    }
    for (id, after) in &new.by_id {
        let unchanged = old
            .by_id
            .get(id)
            .is_some_and(|before| before.profile() == after.profile());
        if !unchanged {
            changes.push(Change::Upsert(Arc::clone(after)));
        }
    }
    changes
}

/// Re-reads SQLite whenever a reload is requested (TUI `changed` over IPC) and publishes
/// the new snapshot. Bursts of requests are coalesced into one read.
pub async fn run_reloader(
    store: Store,
    mut requests: mpsc::UnboundedReceiver<Option<String>>,
    tx: watch::Sender<Arc<Snapshot>>,
    hub: Arc<Hub>,
) {
    let store = Arc::new(Mutex::new(store));
    while let Some(mut by) = requests.recv().await {
        while let Ok(next) = requests.try_recv() {
            by = next.or(by);
        }
        let store = Arc::clone(&store);
        let users = tokio::task::spawn_blocking(move || store.lock().unwrap().list()).await;
        match users {
            Ok(Ok(users)) => {
                let snap = Snapshot::from_users(users);
                tracing::info!(
                    "DB recargada — {} usuarios ({} activos)",
                    snap.total,
                    snap.len()
                );
                hub.emit(ServerMsg::Reloaded { users: snap.total, active: snap.len(), by });
                tx.send_replace(Arc::new(snap));
            }
            Ok(Err(e)) => tracing::error!("DB reload failed: {e}"),
            Err(e) => tracing::error!("DB reload task: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: i64, slug: &str, enabled: bool) -> User {
        User {
            id,
            slug: slug.into(),
            display_name: slug.into(),
            twitch_channel: format!("chan_{slug}"),
            idvk: "id1".into(),
            enabled,
            commands: vec!["#vk".into(), "#OKRU".into()],
            whitelist: vec!["Friend".into()],
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn kinds(changes: &[Change]) -> Vec<String> {
        let mut out: Vec<String> = changes
            .iter()
            .map(|c| match c {
                Change::Upsert(e) => format!("+{}", e.user.slug),
                Change::Remove(s) => format!("-{s}"),
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn matches_aliases_and_whitelist() {
        let snap = Snapshot::from_users(vec![user(1, "a", true), user(2, "b", false)]);
        let entry = snap.by_channel("chan_a").unwrap();
        assert!(entry.matches_command("#okru \u{034F}"));
        assert!(entry.matches_command(" #VK"));
        assert!(!entry.matches_command("#vk hola"));
        assert!(entry.is_whitelisted("friend"));
        assert!(snap.by_channel("chan_b").is_none());
        assert_eq!(snap.total, 2);
    }

    #[test]
    fn diffs_snapshots() {
        let old = Snapshot::from_users(vec![user(1, "a", true), user(2, "b", true), user(3, "c", true)]);

        let mut renamed = user(1, "a2", true);
        renamed.twitch_channel = "chan_a".into();
        let mut whitelist_only = user(2, "b", true);
        whitelist_only.whitelist = vec!["other".into()];
        let new = Snapshot::from_users(vec![
            renamed,
            whitelist_only,
            user(3, "c", false),
            user(4, "d", true),
        ]);

        assert_eq!(kinds(&diff(&old, &new)), vec!["+a2", "+d", "-a", "-c"]);
        assert!(diff(&new, &new).is_empty());
    }
}
