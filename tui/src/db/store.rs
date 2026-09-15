use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, ErrorCode, OptionalExtension, Transaction};

use super::model::{Field, FieldError, User, UserDraft};

/// Ordered migrations; index + 1 == `PRAGMA user_version` after applying.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE users (
    id             INTEGER PRIMARY KEY,
    slug           TEXT NOT NULL UNIQUE,
    display_name   TEXT NOT NULL,
    twitch_channel TEXT NOT NULL UNIQUE,
    idvk           TEXT NOT NULL,
    enabled        INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE user_commands (
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    alias    TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (user_id, alias)
);
CREATE TABLE user_whitelist (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    login   TEXT NOT NULL,
    PRIMARY KEY (user_id, login)
);
"#];

#[derive(Debug)]
pub enum StoreError {
    Invalid(Vec<FieldError>),
    /// UNIQUE constraint on this field.
    Conflict(Field),
    NotFound,
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Invalid(errs) => {
                let list: Vec<String> = errs.iter().map(ToString::to_string).collect();
                write!(f, "datos inválidos: {}", list.join("; "))
            }
            StoreError::Conflict(field) => write!(f, "ya existe un usuario con ese {field}"),
            StoreError::NotFound => f.write_str("usuario no encontrado"),
            StoreError::Sqlite(e) => write!(f, "sqlite: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(err, Some(msg)) = &e {
            if err.code == ErrorCode::ConstraintViolation {
                if msg.contains("users.slug") {
                    return StoreError::Conflict(Field::Slug);
                }
                if msg.contains("users.twitch_channel") {
                    return StoreError::Conflict(Field::TwitchChannel);
                }
            }
        }
        StoreError::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating dirs + file if needed) and migrates.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> anyhow::Result<()> {
        let version: usize = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n == 0)
    }

    /// All users ordered by slug, with commands + whitelist.
    pub fn list(&self) -> Result<Vec<User>> {
        let mut commands = self.child_map(
            "SELECT user_id, alias FROM user_commands ORDER BY user_id, position",
        )?;
        let mut whitelist =
            self.child_map("SELECT user_id, login FROM user_whitelist ORDER BY user_id, login")?;

        let mut stmt = self.conn.prepare(
            "SELECT id, slug, display_name, twitch_channel, idvk, enabled, created_at, updated_at
             FROM users ORDER BY slug",
        )?;
        let users = stmt
            .query_map([], |r| {
                Ok(User {
                    id: r.get(0)?,
                    slug: r.get(1)?,
                    display_name: r.get(2)?,
                    twitch_channel: r.get(3)?,
                    idvk: r.get(4)?,
                    enabled: r.get(5)?,
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                    commands: Vec::new(),
                    whitelist: Vec::new(),
                })
            })?
            .map(|u| {
                u.map(|mut u| {
                    u.commands = commands.remove(&u.id).unwrap_or_default();
                    u.whitelist = whitelist.remove(&u.id).unwrap_or_default();
                    u
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(users)
    }

    pub fn get(&self, id: i64) -> Result<User> {
        self.list()?
            .into_iter()
            .find(|u| u.id == id)
            .ok_or(StoreError::NotFound)
    }

    pub fn insert(&mut self, draft: &UserDraft) -> Result<i64> {
        let d = validated(draft)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO users (slug, display_name, twitch_channel, idvk, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![d.slug, d.display_name, d.twitch_channel, d.idvk, d.enabled],
        )?;
        let id = tx.last_insert_rowid();
        write_children(&tx, id, &d)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn update(&mut self, id: i64, draft: &UserDraft) -> Result<()> {
        let d = validated(draft)?;
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE users SET slug = ?1, display_name = ?2, twitch_channel = ?3, idvk = ?4,
                    enabled = ?5, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ?6",
            params![d.slug, d.display_name, d.twitch_channel, d.idvk, d.enabled, id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        tx.execute("DELETE FROM user_commands WHERE user_id = ?1", [id])?;
        tx.execute("DELETE FROM user_whitelist WHERE user_id = ?1", [id])?;
        write_children(&tx, id, &d)?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_enabled(&mut self, id: i64, enabled: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE users SET enabled = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ?2",
            params![enabled, id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub fn delete(&mut self, id: i64) -> Result<()> {
        let changed = self.conn.execute("DELETE FROM users WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub fn find_by_slug(&self, slug: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT id FROM users WHERE slug = ?1", [slug], |r| r.get(0))
            .optional()?)
    }

    fn child_map(&self, sql: &str) -> Result<HashMap<i64, Vec<String>>> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut map: HashMap<i64, Vec<String>> = HashMap::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, value) = row?;
            map.entry(id).or_default().push(value);
        }
        Ok(map)
    }
}

fn validated(draft: &UserDraft) -> Result<UserDraft> {
    let errors = draft.validate();
    if !errors.is_empty() {
        return Err(StoreError::Invalid(errors));
    }
    Ok(draft.normalized())
}

fn write_children(tx: &Transaction<'_>, id: i64, d: &UserDraft) -> Result<()> {
    for (pos, alias) in d.commands.iter().enumerate() {
        tx.execute(
            "INSERT INTO user_commands (user_id, alias, position) VALUES (?1, ?2, ?3)",
            params![id, alias, pos as i64],
        )?;
    }
    for login in &d.whitelist {
        tx.execute(
            "INSERT INTO user_whitelist (user_id, login) VALUES (?1, ?2)",
            params![id, login],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(slug: &str, channel: &str) -> UserDraft {
        UserDraft {
            slug: slug.into(),
            display_name: slug.to_uppercase(),
            twitch_channel: channel.into(),
            idvk: "id1".into(),
            enabled: true,
            commands: vec!["#vk".into(), "#okru".into()],
            whitelist: vec!["user1".into()],
        }
    }

    #[test]
    fn crud_roundtrip() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(store.is_empty().unwrap());

        let id = store.insert(&draft("uno", "canal_uno")).unwrap();
        let user = store.get(id).unwrap();
        assert_eq!(user.commands, vec!["#vk", "#okru"]);
        assert_eq!(user.whitelist, vec!["user1"]);

        let mut edit = user.to_draft();
        edit.commands = vec!["!web".into()];
        edit.whitelist.clear();
        store.update(id, &edit).unwrap();
        let user = store.get(id).unwrap();
        assert_eq!(user.commands, vec!["!web"]);
        assert!(user.whitelist.is_empty());

        store.set_enabled(id, false).unwrap();
        assert!(!store.get(id).unwrap().enabled);
        assert_eq!(store.find_by_slug("uno").unwrap(), Some(id));

        store.delete(id).unwrap();
        assert!(matches!(store.get(id), Err(StoreError::NotFound)));
    }

    #[test]
    fn maps_unique_conflicts_and_validation() {
        let mut store = Store::open_in_memory().unwrap();
        store.insert(&draft("uno", "canal_uno")).unwrap();
        assert!(matches!(
            store.insert(&draft("uno", "canal_dos")),
            Err(StoreError::Conflict(Field::Slug))
        ));
        assert!(matches!(
            store.insert(&draft("dos", "canal_uno")),
            Err(StoreError::Conflict(Field::TwitchChannel))
        ));
        assert!(matches!(
            store.insert(&draft("api", "canal_tres")),
            Err(StoreError::Invalid(_))
        ));
    }
}
