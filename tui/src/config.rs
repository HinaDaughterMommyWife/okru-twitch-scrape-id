//! The part of the backend's config.toml that okru-tui also reads.

use std::path::Path;

use serde::Deserialize;

use crate::ipc::DEFAULT_PORT;

/// Local Astro dev server; production configs set `webURL = "https://watch.shonensemanal.site"`.
pub const DEFAULT_WEB_URL: &str = "http://localhost:4321";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SharedConfig {
    /// Local TCP link with okru-backend.
    #[serde(rename = "ipcPort", default = "default_ipc_port")]
    pub ipc_port: u16,
    /// Public web base, used to show each user's page / VODs links.
    #[serde(rename = "webURL", default = "default_web_url")]
    pub web_url: String,
}

fn default_ipc_port() -> u16 {
    DEFAULT_PORT
}

fn default_web_url() -> String {
    DEFAULT_WEB_URL.into()
}

impl Default for SharedConfig {
    fn default() -> Self {
        Self { ipc_port: DEFAULT_PORT, web_url: default_web_url() }
    }
}

impl SharedConfig {
    /// Missing file → defaults; missing keys → their defaults.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) => Ok(toml::from_str(&raw)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// `https://site/` → `https://site`
    pub fn web_base(&self) -> &str {
        self.web_url.trim().trim_end_matches('/')
    }

    pub fn page_url(&self, slug: &str) -> String {
        format!("{}/{slug}", self.web_base())
    }

    pub fn vods_url(&self, slug: &str) -> String {
        format!("{}/vods", self.page_url(slug))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_defaults_and_overrides() {
        let dir = std::env::temp_dir().join(format!("okru-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        assert_eq!(SharedConfig::load(&path).unwrap(), SharedConfig::default());

        std::fs::write(
            &path,
            "botName = \"x\"\nipcPort = 31000\nwebURL = \"https://watch.shonensemanal.site/\"\n",
        )
        .unwrap();
        let cfg = SharedConfig::load(&path).unwrap();
        assert_eq!(cfg.ipc_port, 31000);
        assert_eq!(cfg.page_url("oguriuzal_jolyne"), "https://watch.shonensemanal.site/oguriuzal_jolyne");
        assert_eq!(cfg.vods_url("oguriuzal_jolyne"), "https://watch.shonensemanal.site/oguriuzal_jolyne/vods");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
