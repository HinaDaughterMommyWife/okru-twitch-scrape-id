//! Twitch IRC bot via twitch-irc crate.
//! - Command: `#vk` (mods + broadcaster only)
//! - Debounce 40s: first `#vk` runs; further `#vk` from any mod ignored until done or 40s
//! - Safe-send: respects slow-mode + emote-only from ROOMSTATE; action always runs even if send fails
//! - Token refresh handled by RefreshingLoginCredentials

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use twitch_irc::login::RefreshingLoginCredentials;
use twitch_irc::message::ServerMessage;
use twitch_irc::{ClientConfig, SecureTCPTransport, TwitchIRCClient};

use crate::config::Config;
use crate::credentials::FileTokenStorage;
use crate::scheduler::Scheduler;
use crate::vk;
use crate::worker_client;

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

struct BotHandle {
    checking: AtomicBool,
    check_start: Mutex<Option<Instant>>,
}

impl BotHandle {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            checking: AtomicBool::new(false),
            check_start: Mutex::new(None),
        })
    }

    /// Returns true if this `#vk` should run.
    /// While a check is in flight and < DEBOUNCE_WINDOW have elapsed, all other `#vk` are ignored
    /// (any moderator). After that the lock is force-released so a stuck check can't block forever.
    async fn try_begin_check(&self) -> bool {
        if self.checking.load(Ordering::SeqCst) {
            let start = self.check_start.lock().await;
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
        *self.check_start.lock().await = Some(Instant::now());
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

fn is_privileged(msg: &twitch_irc::message::PrivmsgMessage) -> bool {
    msg.badges
        .iter()
        .any(|b| b.name == "moderator" || b.name == "broadcaster")
}

/// Invisible / format chars that Twitch clients sometimes append (e.g. U+034F).
fn is_invisible_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}' // soft hyphen
                | '\u{034F}' // combining grapheme joiner — seen in chat as `#vk ͏`
                | '\u{061C}' // arabic letter mark
                | '\u{180E}' // mongolian vowel separator
                | '\u{200B}'..= '\u{200F}' // zwsp, zwnj, zwj, lrm, rlm
                | '\u{202A}'..= '\u{202E}' // bidi overrides
                | '\u{2060}'..= '\u{2064}' // word joiner, etc.
                | '\u{2066}'..= '\u{206F}'
                | '\u{FEFF}' // bom / zwnbsp
                | '\u{E0000}'..= '\u{E007F}' // tags
        )
}

/// True for `#vk` ignoring case, whitespace and invisible junk.
fn is_vk_command(raw: &str) -> bool {
    let cleaned: String = raw
        .chars()
        .filter(|&c| !c.is_whitespace() && !is_invisible_char(c))
        .flat_map(|c| c.to_lowercase())
        .collect();
    cleaned == "#vk"
}

async fn run_check(
    http: &reqwest::Client,
    cfg: &Config,
    client: &Client,
    channel: &str,
    chat: &Arc<Mutex<ChatState>>,
) {
    match tokio::time::timeout(CHECK_TIMEOUT, async {
        let (found, oid, vid) = vk::check_and_ids(http, &cfg.idvk).await?;
        worker_client::post_to_worker(http, &cfg.post_url, &cfg.post_auth, &oid, &vid).await?;
        Ok::<_, anyhow::Error>(found)
    })
    .await
    {
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
            tracing::error!("Check error: {e:#}");
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
            tracing::warn!("Check timed out after {}s", CHECK_TIMEOUT.as_secs());
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

/// Connect to Twitch IRC and process messages until the receiver closes.
pub async fn run_bot(
    cfg: Arc<Config>,
    http: reqwest::Client,
    scheduler: Arc<Scheduler>,
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

    let channel = cfg.channel_login();
    client
        .join(channel.clone())
        .context("join channel")?;

    tracing::info!(
        "Bot ready — nick={}, channel=#{}",
        cfg.bot_login(),
        channel
    );

    let handle = BotHandle::new();
    let chat = Arc::new(Mutex::new(ChatState::new()));

    while let Some(message) = incoming.recv().await {
        match message {
            ServerMessage::RoomState(rs) => {
                chat.lock().await.apply_roomstate(&rs);
            }
            ServerMessage::Notice(n) => {
                chat.lock()
                    .await
                    .apply_notice(n.message_id.as_deref(), &n.message_text);
                tracing::debug!("NOTICE {:?}: {}", n.channel_login, n.message_text);
            }
            ServerMessage::Privmsg(msg) => {
                if !is_vk_command(&msg.message_text) {
                    // Help diagnose near-misses like `#vk` + invisible chars
                    let lower = msg.message_text.to_lowercase();
                    if lower.contains("vk") && lower.contains('#') {
                        tracing::debug!(
                            "Near-miss #vk ignored raw={:?} from {}",
                            msg.message_text,
                            msg.sender.login
                        );
                    }
                    continue;
                }

                if !is_privileged(&msg) {
                    tracing::debug!(
                        "Ignored #vk from non-privileged user: {}",
                        msg.sender.login
                    );
                    continue;
                }

                if !handle.try_begin_check().await {
                    tracing::info!(
                        "Debounce: check in progress (<{}s), ignoring #vk from {}",
                        DEBOUNCE_WINDOW.as_secs(),
                        msg.sender.login
                    );
                    continue;
                }

                // Activate / refresh the 8h interval window
                scheduler.bump().await;

                let chan = msg.channel_login.clone();
                let http = http.clone();
                let cfg = Arc::clone(&cfg);
                let client = client.clone();
                let handle = Arc::clone(&handle);
                let chat = Arc::clone(&chat);

                // Spawn so the IRC loop stays free; keep greeting → check order.
                tokio::spawn(async move {
                    safe_send(
                        &client,
                        &chan,
                        &chat,
                        "Buscando streaming... espera un momento 👀",
                        Some("TTours"),
                    )
                    .await;
                    run_check(&http, &cfg, &client, &chan, &chat).await;
                    handle.end_check();
                });
            }
            _ => {}
        }
    }

    Ok(())
}
