//! SQLite storage for okru users (channels).
//!
//! Used by the TUI binary (CRUD) and by `okru-backend` (reads + hot-reload, via
//! `okru-tui = { default-features = false }`), so validation and schema live in one place.

mod model;
mod paths;
mod store;

pub use model::{
    clean_command, is_invisible_char, parse_csv, Field, FieldError, User, UserDraft,
    RESERVED_SLUGS,
};
pub use paths::{default_config_path, default_db_path, exe_dir};
pub use store::{Store, StoreError};
