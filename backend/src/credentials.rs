//! Twitch OAuth credentials persistence for twitch-irc RefreshingLoginCredentials.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use twitch_irc::login::{TokenStorage, UserAccessToken};

use crate::config::credentials_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredentials {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl StoredCredentials {
    pub fn from_oauth_response(data: &serde_json::Value) -> Result<Self> {
        let access_token = data
            .get("access_token")
            .and_then(|v| v.as_str())
            .context("oauth response missing access_token")?
            .to_string();
        let refresh_token = data
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .context("oauth response missing refresh_token")?
            .to_string();
        let expires_in = data.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(0);
        let now = chrono::Utc::now();
        Ok(Self {
            access_token,
            refresh_token,
            created_at: Some(now),
            expires_at: if expires_in > 0 {
                Some(now + chrono::Duration::seconds(expires_in as i64))
            } else {
                None
            },
        })
    }

    pub fn to_user_access_token(&self) -> UserAccessToken {
        UserAccessToken {
            access_token: self.access_token.clone(),
            refresh_token: self.refresh_token.clone(),
            created_at: self.created_at.unwrap_or_else(chrono::Utc::now),
            expires_at: self.expires_at,
        }
    }

    pub fn from_user_access_token(token: &UserAccessToken) -> Self {
        Self {
            access_token: token.access_token.clone(),
            refresh_token: token.refresh_token.clone(),
            created_at: Some(token.created_at),
            expires_at: token.expires_at,
        }
    }
}

pub fn credentials_exist() -> bool {
    credentials_path().exists()
}

#[allow(dead_code)]
pub fn load_credentials() -> Result<Option<StoredCredentials>> {
    let path = credentials_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("leer {}", path.display()))?;
    let creds: StoredCredentials = serde_json::from_str(&raw)
        .with_context(|| format!("parsear {}", path.display()))?;
    Ok(Some(creds))
}

pub fn save_credentials(creds: &StoredCredentials) -> Result<()> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(creds)?;
    fs::write(&path, raw).with_context(|| format!("escribir {}", path.display()))?;
    Ok(())
}

pub fn clear_credentials() -> Result<()> {
    let path = credentials_path();
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("borrar {}", path.display()))?;
    }
    Ok(())
}

/// File-backed TokenStorage for twitch-irc RefreshingLoginCredentials.
#[derive(Debug, Clone)]
pub struct FileTokenStorage {
    path: PathBuf,
    inner: Arc<Mutex<()>>,
}

impl FileTokenStorage {
    pub fn new() -> Self {
        Self {
            path: credentials_path(),
            inner: Arc::new(Mutex::new(())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct StorageError(String);

#[async_trait]
impl TokenStorage for FileTokenStorage {
    type LoadError = StorageError;
    type UpdateError = StorageError;

    async fn load_token(&mut self) -> Result<UserAccessToken, Self::LoadError> {
        let _guard = self.inner.lock().await;
        let raw = fs::read_to_string(&self.path).map_err(|e| {
            StorageError(format!("load {}: {e}", self.path.display()))
        })?;
        let creds: StoredCredentials = serde_json::from_str(&raw).map_err(|e| {
            StorageError(format!("parse {}: {e}", self.path.display()))
        })?;
        Ok(creds.to_user_access_token())
    }

    async fn update_token(&mut self, token: &UserAccessToken) -> Result<(), Self::UpdateError> {
        let _guard = self.inner.lock().await;
        let creds = StoredCredentials::from_user_access_token(token);
        let raw = serde_json::to_string_pretty(&creds)
            .map_err(|e| StorageError(format!("serialize token: {e}")))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| StorageError(e.to_string()))?;
        }
        fs::write(&self.path, raw).map_err(|e| {
            StorageError(format!("write {}: {e}", self.path.display()))
        })?;
        tracing::info!("Twitch token refreshed and persisted");
        Ok(())
    }
}
