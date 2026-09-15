//! Twitch IRC bot via twitch-irc crate — one connection, many channels.
//! - Channels, command aliases and whitelist come from the SQLite snapshot (hot-reloaded)
//! - Commands: per-user aliases (mods + broadcaster + whitelisted users)
//! - Debounce 40s per channel: first command runs; further ones ignored until done or 40s
//! - Safe-send: respects slow-mode + emote-only from ROOMSTATE; action always runs even if send fails
//! - Token refresh handled by RefreshingLoginCredentials

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use twitch_irc::login::RefreshingLoginCredentials;
use twitch_irc::message::ServerMessage;
use twitch_irc::{ClientConfig, SecureTCPTransport, TwitchIRCClient};

use crate::check::run_check;
use crate::config::Config;
use crate::credentials::FileTokenStorage;
use crate::ipc::Hub;
use okru_tui::ipc::{CheckOutcome, Role, ServerMsg};
use crate::registry::{SnapshotRx, UserEntry};
use crate::scheduler::Scheduler;
use crate::worker_client::WorkerClient;

const DEBOUNCE_WINDOW: Duration = Duration::from_secs(40);
const CHECK_TIMEOUT: Duration = Duration::from_secs(40);

type Creds = RefreshingLoginCredentials<FileTokenStorage>;
type Client = TwitchIRCClient<SecureTCPTransport, Creds>;

/// Live chat constraints from Twitch ROOMSTATE / NOTICE.
#[derive(Debug)]
struct ChatState {
    emote_only: bool,
    /// Zero = slow mode off.
    slow_mode: Duration,
    chat_disabled: bool,
    last_msg_time: Option<Instant>,
}

impl ChatState {
    fn new() -> Self {
        Self {
            emote_only: false,
            slow_mode: Duration::ZERO,
            chat_disabled: false,
            last_msg_time: None,
        }
    }

    fn apply_roomstate(&mut self, msg: &twitch_irc::message::RoomStateMessage) {
        if let Some(v) = msg.emote_only {
            self.emote_only = v;
            tracing::info!("ROOMSTATE emote_only={v}");
        }
        if let Some(d) = msg.slow_mode {
            // Twitch sends 0s for /slowoff
            self.slow_mode = d;
            tracing::info!("ROOMSTATE slow_mode={}s", d.as_secs());
        }
    }

    fn apply_notice(&mut self, msg_id: Option<&str>, text: &str) {
        match msg_id {
            Some("msg_rejected") | Some("msg_rejected_mandatory") => {
                tracing::warn!("NOTICE reject: {text}");
            }
            Some("msg_banned") | Some("msg_channel_suspended") | Some("tos_ban") => {
                self.chat_disabled = true;
                tracing::warn!("Chat disabled via NOTICE ({msg_id:?}): {text}");
            }
            Some("emote_only_on") => {
                self.emote_only = true;
                tracing::info!("NOTICE emote_only=on");
            }
            Some("emote_only_off") => {
                self.emote_only = false;
                tracing::info!("NOTICE emote_only=off");
            }
            Some("slow_on") => {
                // Duration comes from ROOMSTATE; keep a safe default if unknown.
                if self.slow_mode.is_zero() {
                    self.slow_mode = Duration::from_secs(5);
                }
                tracing::info!("NOTICE slow=on ({}s)", self.slow_mode.as_secs());
            }
            Some("slow_off") => {
                self.slow_mode = Duration::ZERO;
                tracing::info!("NOTICE slow=off");
            }
            _ => {}
        }
    }
}

/// Per-channel runtime: chat constraints + command debounce.
struct ChannelRuntime {
    chat: Mutex<ChatState>,
    checking: AtomicBool,
    check_start: std::sync::Mutex<Option<Instant>>,
}

impl ChannelRuntime {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            chat: Mutex::new(ChatState::new()),
            checking: AtomicBool::new(false),
            check_start: std::sync::Mutex::new(None),
        })
    }

    /// Returns true if this command should run.
    /// While a check is in flight and < DEBOUNCE_WINDOW have elapsed, all other commands in this
    /// channel are ignored. After that the lock is force-released so a stuck check can't block forever.
    fn try_begin_check(&self) -> bool {
        let mut start = self.check_start.lock().unwrap();
        if self.checking.load(Ordering::SeqCst) {
            if let Some(t) = *start {
                if t.elapsed() < DEBOUNCE_WINDOW {
                    return false;
                }
                tracing::warn!(
                    "Debounce force-release after {}s (previous check still marked in-flight)",
                    t.elapsed().as_secs()
                );
            }
        }
        self.checking.store(true, Ordering::SeqCst);
        *start = Some(Instant::now());
        true
    }

    fn end_check(&self) {
        self.checking.store(false, Ordering::SeqCst);
    }
}

/// Safe chat send — never aborts scrape/post.
/// - chat disabled → skip silently
/// - emote-only → use `emote_text` when provided
/// - slow mode → wait remaining delay before send
async fn safe_send(
    client: &Client,
    channel: &str,
    chat: &Mutex<ChatState>,
    text: &str,
    emote_text: Option<&str>,
) {
    let (msg, wait) = {
        let st = chat.lock().await;
        if st.chat_disabled {
            tracing::info!("Chat disabled — silent, skipping: {text}");
            return;
        }

        let msg = if st.emote_only {
            match emote_text {
                Some(e) => e.to_string(),
                None => {
                    tracing::info!("Emote-only — no emote fallback, skipping: {text}");
                    return;
                }
            }
        } else {
            text.to_string()
        };

        let wait = if st.slow_mode > Duration::ZERO {
            match st.last_msg_time {
                Some(t) if t.elapsed() < st.slow_mode => st.slow_mode - t.elapsed(),
                _ => Duration::ZERO,
            }
        } else {
            Duration::ZERO
        };

        (msg, wait)
    };

    if wait > Duration::ZERO {
        tracing::info!("Slow mode — waiting {:.1}s before send", wait.as_secs_f32());
        tokio::time::sleep(wait).await;
    }

    match client.say(channel.to_string(), msg.clone()).await {
        Ok(()) => {
            chat.lock().await.last_msg_time = Some(Instant::now());
            tracing::info!("Sent: {msg}");
        }
        Err(e) => tracing::warn!("Send failed (non-fatal): {e}"),
    }
}

/// Why this chat user may trigger commands (`None` = not allowed).
fn privilege(msg: &twitch_irc::message::PrivmsgMessage, entry: &UserEntry) -> Option<Role> {
    let has_badge = |name: &str| msg.badges.iter().any(|b| b.name == name);
    if has_badge("broadcaster") {
        Some(Role::Broadcaster)
    } else if has_badge("moderator") {
        Some(Role::Mod)
    } else if entry.is_whitelisted(&msg.sender.login) {
        Some(Role::Whitelist)
    } else {
        None
    }
}

async fn reply_check(
    http: &reqwest::Client,
    worker: &WorkerClient,
    entry: &UserEntry,
    client: &Client,
    runtime: &ChannelRuntime,
    hub: &Hub,
) {
    let channel = &entry.user.twitch_channel;
    let chat = &runtime.chat;
    let result = tokio::time::timeout(CHECK_TIMEOUT, run_check(http, worker, entry)).await;
    let (outcome, detail) = match &result {
        Ok(Ok(true)) => (CheckOutcome::Live, None),
        Ok(Ok(false)) => (CheckOutcome::Offline, None),
        Ok(Err(e)) => (CheckOutcome::Error, Some(format!("{e:#}"))),
        Err(_) => (CheckOutcome::Timeout, None),
    };
    hub.emit(ServerMsg::CheckResult { slug: entry.user.slug.clone(), outcome, detail });

    match result {
        Ok(Ok(true)) => {
            safe_send(
                client,
                channel,
                chat,
                "✅ Stream activo detectado! En unos momentos se actualizará el sitio",
                Some("VoteYea"),
            )
            .await;
        }
        Ok(Ok(false)) => {
            safe_send(
                client,
                channel,
                chat,
                "📭 No hay stream activo en este momento 😭🍍💢",
                Some("VoteNay"),
            )
            .await;
        }
        Ok(Err(e)) => {
            tracing::error!("[{}] Check error: {e:#}", entry.user.slug);
            safe_send(
                client,
                channel,
                chat,
                "❌ Error durante la búsqueda NotLikeThis",
                Some("VoteNay"),
            )
            .await;
        }
        Err(_) => {
            tracing::warn!(
                "[{}] Check timed out after {}s",
                entry.user.slug,
                CHECK_TIMEOUT.as_secs()
            );
            safe_send(
                client,
                channel,
                chat,
                "⏱️ La búsqueda tardó demasiado, intenta de nuevo.",
                Some("VoteNay"),
            )
            .await;
        }
    }
}

fn join_channels(client: &Client, previous: &HashSet<String>, wanted: &HashSet<String>) {
    let sorted = |set: &HashSet<String>| {
        let mut v: Vec<String> = set.iter().cloned().collect();
        v.sort();
        v
    };
    let added: HashSet<String> = wanted.difference(previous).cloned().collect();
    let removed: HashSet<String> = previous.difference(wanted).cloned().collect();
    tracing::info!(
        "Canales: {:?} (+{:?} -{:?})",
        sorted(wanted),
        sorted(&added),
        sorted(&removed)
    );
    if let Err(e) = client.set_wanted_channels(wanted.clone()) {
        tracing::error!("set_wanted_channels failed: {e}");
    }
}

fn runtime_for(runtimes: &mut HashMap<String, Arc<ChannelRuntime>>, channel: &str) -> Arc<ChannelRuntime> {
    Arc::clone(
        runtimes
            .entry(channel.to_string())
            .or_insert_with(ChannelRuntime::new),
    )
}

/// Connect to Twitch IRC and process messages until the receiver closes.
pub async fn run_bot(
    cfg: Arc<Config>,
    http: reqwest::Client,
    worker: WorkerClient,
    scheduler: Arc<Scheduler>,
    mut rx: SnapshotRx,
    hub: Arc<Hub>,
) -> Result<()> {
    let storage = FileTokenStorage::new();
    let credentials = RefreshingLoginCredentials::init_with_username(
        Some(cfg.bot_login()),
        cfg.twitch_token_id.clone(),
        cfg.twitch_token_secret.clone(),
        storage,
    );

    let config = ClientConfig::new_simple(credentials);
    let (mut incoming, client) = TwitchIRCClient::<SecureTCPTransport, Creds>::new(config);

    let bot_login = cfg.bot_login();
    let mut channels = rx.borrow_and_update().channels();
    join_channels(&client, &HashSet::new(), &channels);
    tracing::info!("Bot ready — nick={bot_login}");

    let mut runtimes: HashMap<String, Arc<ChannelRuntime>> = HashMap::new();

    loop {
        let message = tokio::select! {
            message = incoming.recv() => match message {
                Some(m) => m,
                None => break,
            },
            changed = rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let next = rx.borrow_and_update().channels();
                if next != channels {
                    join_channels(&client, &channels, &next);
                    runtimes.retain(|c, _| next.contains(c));
                    channels = next;
                }
                continue;
            }
        };

        match message {
            // Twitch confirms our own JOIN/PART: proof the bot is really in the channel.
            ServerMessage::Join(j) if j.user_login == bot_login => {
                tracing::info!("✓ JOIN #{}", j.channel_login);
                hub.emit(ServerMsg::Joined { channel: j.channel_login });
            }
            ServerMessage::Part(p) if p.user_login == bot_login => {
                tracing::info!("✓ PART #{}", p.channel_login);
                hub.emit(ServerMsg::Parted { channel: p.channel_login });
            }
            ServerMessage::RoomState(rs) => {
                let runtime = runtime_for(&mut runtimes, &rs.channel_login);
                runtime.chat.lock().await.apply_roomstate(&rs);
            }
            ServerMessage::Notice(n) => {
                tracing::debug!("NOTICE {:?}: {}", n.channel_login, n.message_text);
                if let Some(channel) = &n.channel_login {
                    let runtime = runtime_for(&mut runtimes, channel);
                    runtime
                        .chat
                        .lock()
                        .await
                        .apply_notice(n.message_id.as_deref(), &n.message_text);
                }
            }
            ServerMessage::Privmsg(msg) => {
                // Read the latest snapshot per message: alias/whitelist edits apply instantly.
                let Some(entry) = rx.borrow().by_channel(&msg.channel_login) else {
                    continue;
                };
                if !entry.matches_command(&msg.message_text) {
                    continue;
                }
                let slug = entry.user.slug.clone();

                let Some(role) = privilege(&msg, &entry) else {
                    tracing::debug!(
                        "[{slug}] Ignored command from non-privileged user: {}",
                        msg.sender.login
                    );
                    continue;
                };

                let runtime = runtime_for(&mut runtimes, &msg.channel_login);
                let accepted = runtime.try_begin_check();
                hub.emit(ServerMsg::CommandUsed {
                    slug: slug.clone(),
                    channel: msg.channel_login.clone(),
                    user: msg.sender.login.clone(),
                    role,
                    command: msg.message_text.trim().to_string(),
                    accepted,
                });
                if !accepted {
                    tracing::info!(
                        "[{slug}] Debounce: check in progress (<{}s), ignoring command from {}",
                        DEBOUNCE_WINDOW.as_secs(),
                        msg.sender.login
                    );
                    continue;
                }

                tracing::info!(
                    "[{slug}] Command {:?} from {} ({})",
                    msg.message_text.trim(),
                    msg.sender.login,
                    role.label()
                );
                // Activate / refresh the 8h interval window for this user
                scheduler.bump(entry.user.id, &slug);

                let http = http.clone();
                let worker = worker.clone();
                let client = client.clone();
                let hub = Arc::clone(&hub);

                // Spawn so the IRC loop stays free; keep greeting → check order.
                tokio::spawn(async move {
                    safe_send(
                        &client,
                        &entry.user.twitch_channel,
                        &runtime.chat,
                        "Buscando streaming... espera un momento 👀",
                        Some("TTours"),
                    )
                    .await;
                    reply_check(&http, &worker, &entry, &client, &runtime, &hub).await;
                    runtime.end_check();
                });
            }
            _ => {}
        }
    }

    Ok(())
}
