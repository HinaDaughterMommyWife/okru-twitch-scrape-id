use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use okru_tui::ipc::DEFAULT_PORT as DEFAULT_IPC_PORT;

const TEMPLATE: &str = r#"# Copy / edit this file next to the okru-backend binary and fill in values.
# Channels (slug, twitch channel, idvk, command aliases, whitelist) live in SQLite — use okru-tui.

intervalo = 60                 # minutes between loop ticks (once a command activates the 8h window)

twitchTokenId = ""             # Twitch application client id
twitchTokenSecret = ""         # Twitch application client secret
botName = "comomegustapadreball"

# === Worker (Cloudflare) base URL + Basic Auth token ===
workerURL = "http://localhost:8787"
postAuth = ""

# === Local HTTP server (health + OAuth setup) ===
setupPathKey = ""              # secret path segment; leave empty to auto-generate on first run
port = 9622
baseUrl = "http://localhost:9622"

# === Shared with okru-tui (it reads this same file) ===
ipcPort = 29622                        # local TCP link, 127.0.0.1 only
webURL = "http://localhost:4321"       # public web base (prod: https://watch.shonensemanal.site)
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub intervalo: u64,
    #[serde(rename = "twitchTokenId")]
    pub twitch_token_id: String,
    #[serde(rename = "twitchTokenSecret")]
    pub twitch_token_secret: String,
    #[serde(rename = "botName")]
    pub bot_name: String,
    /// Worker base URL, e.g. `http://localhost:8787`.
    #[serde(rename = "workerURL")]
    pub worker_url: String,
    #[serde(rename = "postAuth")]
    pub post_auth: String,
    #[serde(rename = "setupPathKey", default)]
    pub setup_path_key: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(rename = "baseUrl", default = "default_base_url")]
    pub base_url: String,
    /// Local TCP port where okru-tui notifies changes (bound to 127.0.0.1).
    #[serde(rename = "ipcPort", default = "default_ipc_port")]
    pub ipc_port: u16,
    /// Public web base; shown by okru-tui and in startup logs.
    #[serde(rename = "webURL", default = "default_web_url")]
    pub web_url: String,
}

fn default_port() -> u16 {
    9622
}

fn default_base_url() -> String {
    "http://localhost:9622".into()
}

fn default_ipc_port() -> u16 {
    DEFAULT_IPC_PORT
}

fn default_web_url() -> String {
    okru_tui::config::DEFAULT_WEB_URL.into()
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        let mut missing = Vec::new();
        if self.intervalo == 0 {
            missing.push("intervalo (must be >= 1)");
        }
        if self.twitch_token_id.trim().is_empty() {
            missing.push("twitchTokenId");
        }
        if self.twitch_token_secret.trim().is_empty() {
            missing.push("twitchTokenSecret");
        }
        if self.bot_name.trim().is_empty() {
            missing.push("botName");
        }
        if self.worker_url.trim().is_empty() {
            missing.push("workerURL");
        }
        if self.post_auth.trim().is_empty() {
            missing.push("postAuth");
        }
        if self.ipc_port == 0 || self.ipc_port == self.port {
            missing.push("ipcPort (>= 1 y distinto de port)");
        }
        if !missing.is_empty() {
            bail!(
                "config.toml incompleto — completa: {}",
                missing.join(", ")
            );
        }
        Ok(())
    }

    pub fn bot_login(&self) -> String {
        self.bot_name.trim().to_lowercase()
    }

    /// `http://host/` → `http://host`
    pub fn worker_base(&self) -> String {
        self.worker_url.trim().trim_end_matches('/').to_string()
    }
}

/// `OKRU_CONFIG` or `config.toml` next to the binary (same resolution as okru-tui).
pub fn config_path() -> PathBuf {
    okru_tui::db::default_config_path()
}

/// Lives next to config.toml.
pub fn credentials_path() -> PathBuf {
    config_path()
        .parent()
        .map(|dir| dir.join("credentials.json"))
        .unwrap_or_else(|| PathBuf::from("credentials.json"))
}

/// Load config.toml next to the binary. If missing, write a template and exit guidance.
pub fn load_or_create_template() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        write_template(&path)?;
        bail!(
            "No se encontró config.toml.\n\
             Se creó una plantilla en:\n  {}\n\
             Completa los valores y vuelve a ejecutar el binario.",
            path.display()
        );
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    let mut cfg: Config = toml::from_str(&raw)
        .with_context(|| format!("config.toml inválido en {}", path.display()))?;

    // Auto-generate setupPathKey if empty and persist it.
    if cfg.setup_path_key.trim().is_empty() {
        cfg.setup_path_key = random_path_key();
        persist(&path, &cfg)?;
        tracing::info!(
            "setupPathKey generado automáticamente: {}",
            cfg.setup_path_key
        );
    }

    cfg.validate()?;
    Ok(cfg)
}

fn write_template(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, TEMPLATE)
        .with_context(|| format!("no se pudo escribir plantilla en {}", path.display()))?;
    Ok(())
}

fn persist(path: &Path, cfg: &Config) -> Result<()> {
    let text = toml::to_string_pretty(cfg).context("serializar config.toml")?;
    fs::write(path, text).with_context(|| format!("escribir {}", path.display()))?;
    Ok(())
}

fn random_path_key() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", nanos ^ 0xa5a5_c3c3_dead_beef)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_template() {
        let cfg: Config = toml::from_str(TEMPLATE).unwrap();
        assert_eq!(cfg.worker_base(), "http://localhost:8787");
        assert_eq!(cfg.ipc_port, DEFAULT_IPC_PORT);
    }
}
