use std::path::PathBuf;

const DB_FILE: &str = "okru.db";
const CONFIG_FILE: &str = "config.toml";

/// Directory that contains the executable (or cwd as fallback).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// config.toml shared by backend and TUI (`ipcPort`, …):
/// `OKRU_CONFIG` env var, else `config.toml` next to the binary.
pub fn default_config_path() -> PathBuf {
    std::env::var_os("OKRU_CONFIG")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| exe_dir().join(CONFIG_FILE))
}

/// SQLite location shared by backend and TUI:
/// 1. `OKRU_DB` env var
/// 2. debug builds (dev / `cargo run`): `<workspace>/data/okru.db`
/// 3. release builds (dist/, arm/): `okru.db` next to the binary
pub fn default_db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("OKRU_DB").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if cfg!(debug_assertions) {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../data")).join(DB_FILE)
    } else {
        exe_dir().join(DB_FILE)
    }
}
