use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const TEMPLATE: &str = r#"# Copy / edit this file next to the okru-backend binary and fill in values.

idvk = "id1117440596"          # VK profile/community to scrape
intervalo = 60                 # minutes between loop ticks (once #vk activates the window)

twitchTokenId = ""             # Twitch application client id
twitchTokenSecret = ""         # Twitch application client secret
channelTarget = "thedarkraimola"
botName = "comomegustapadreball"

# === Downstream webhook (where to POST stream IDs) ===
postURL = "http://localhost:8787/streaming"
postAuth = ""

# === Local HTTP server (health + OAuth setup) ===
setupPathKey = ""              # secret path segment; leave empty to auto-generate on first run
port = 9622
baseUrl = "http://localhost:9622"
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub idvk: String,
    pub intervalo: u64,
    #[serde(rename = "twitchTokenId")]
    pub twitch_token_id: String,
    #[serde(rename = "twitchTokenSecret")]
    pub twitch_token_secret: String,
    #[serde(rename = "channelTarget")]
    pub channel_target: String,
    #[serde(rename = "botName")]
    pub bot_name: String,
    #[serde(rename = "postURL")]
    pub post_url: String,
    #[serde(rename = "postAuth")]
    pub post_auth: String,
    #[serde(rename = "setupPathKey", default)]
    pub setup_path_key: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(rename = "baseUrl", default = "default_base_url")]
    pub base_url: String,
}

fn default_port() -> u16 {
    9622
}

fn default_base_url() -> String {
    "http://localhost:9622".into()
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        let mut missing = Vec::new();
        if self.idvk.trim().is_empty() {
            missing.push("idvk");
        }
        if self.intervalo == 0 {
            missing.push("intervalo (must be >= 1)");
        }
        if self.twitch_token_id.trim().is_empty() {
            missing.push("twitchTokenId");
        }
        if self.twitch_token_secret.trim().is_empty() {
            missing.push("twitchTokenSecret");
        }
        if self.channel_target.trim().is_empty() {
            missing.push("channelTarget");
        }
        if self.bot_name.trim().is_empty() {
            missing.push("botName");
        }
        if self.post_url.trim().is_empty() {
            missing.push("postURL");
        }
        if self.post_auth.trim().is_empty() {
            missing.push("postAuth");
        }
        if !missing.is_empty() {
            bail!(
                "config.toml incompleto — completa: {}",
                missing.join(", ")
            );
        }
        Ok(())
    }

    pub fn channel_login(&self) -> String {
        self.channel_target.trim().trim_start_matches('#').to_lowercase()
    }

    pub fn bot_login(&self) -> String {
        self.bot_name.trim().to_lowercase()
    }
}

/// Directory that contains the executable (or cwd as fallback).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn config_path() -> PathBuf {
    exe_dir().join("config.toml")
}

pub fn credentials_path() -> PathBuf {
    exe_dir().join("credentials.json")
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
