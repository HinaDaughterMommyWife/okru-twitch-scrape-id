//! `okru-tui` library side, shared with okru-backend:
//! - `db`: SQLite layer
//! - `ipc`: local TCP protocol between TUI and backend
//! - `config`: config.toml keys both read (`ipcPort`, `webURL`)
//! The ratatui binary lives in `main.rs` (feature `tui`, on by default).

pub mod config;
pub mod db;
pub mod ipc;
