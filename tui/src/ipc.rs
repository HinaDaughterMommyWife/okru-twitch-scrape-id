//! Local TCP link between okru-tui (client) and okru-backend (server).
//!
//! Newline-delimited JSON on `127.0.0.1:<ipcPort>`:
//! - TUI → backend: [`ClientMsg`] (`hello`, `changed` after every DB write)
//! - backend → TUI: [`ServerMsg`] (reloads, JOIN/PART confirmations, chat commands, worker sync results)

use serde::{Deserialize, Serialize};

/// Rarely used, below Linux's ephemeral range (32768+). Override with `ipcPort` in config.toml.
pub const DEFAULT_PORT: u16 = 29622;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// First line after connecting; `client` identifies the sender in `reloaded.by`.
    Hello { client: String },
    /// The client committed a DB change: reload now.
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Sent once on connect.
    Welcome { users: usize, active: usize, channels: Vec<String> },
    /// Backend reloaded the DB; `by` = client that asked (None = backend itself).
    Reloaded { users: usize, active: usize, by: Option<String> },
    /// Twitch confirmed the bot joined / left a channel.
    Joined { channel: String },
    Parted { channel: String },
    /// Worker profile published (`put`) or removed (`delete`).
    Synced { slug: String, action: String },
    SyncFailed { slug: String, error: String },
    /// Someone allowed to (broadcaster / mod / whitelist) used a command alias in chat.
    /// `accepted = false` when ignored because a search was already running in that channel.
    CommandUsed {
        slug: String,
        channel: String,
        user: String,
        role: Role,
        command: String,
        accepted: bool,
    },
    /// Result of the VK search triggered by a command.
    CheckResult { slug: String, outcome: CheckOutcome, detail: Option<String> },
}

/// Why a chat user may trigger commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Broadcaster,
    Mod,
    Whitelist,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Broadcaster => "broadcaster",
            Role::Mod => "mod",
            Role::Whitelist => "whitelist",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Live,
    Offline,
    Error,
    Timeout,
}

/// One JSON line (with trailing `\n`).
pub fn encode<T: Serialize>(msg: &T) -> String {
    let mut line = serde_json::to_string(msg).expect("ipc message serializes");
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_roundtrip_as_tagged_json() {
        let line = encode(&ServerMsg::Joined { channel: "perghor_adamia".into() });
        assert_eq!(line, "{\"type\":\"joined\",\"channel\":\"perghor_adamia\"}\n");
        let back: ClientMsg = serde_json::from_str(r#"{"type":"changed"}"#).unwrap();
        assert_eq!(back, ClientMsg::Changed);

        let used = ServerMsg::CommandUsed {
            slug: "a".into(),
            channel: "a".into(),
            user: "friend".into(),
            role: Role::Whitelist,
            command: "#okru".into(),
            accepted: true,
        };
        let json = encode(&used);
        assert!(json.contains("\"type\":\"command_used\"") && json.contains("\"role\":\"whitelist\""));
        assert_eq!(serde_json::from_str::<ServerMsg>(&json).unwrap(), used);
    }
}
