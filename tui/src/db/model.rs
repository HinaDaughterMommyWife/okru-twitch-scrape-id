use std::collections::HashSet;
use std::fmt;

/// Slugs that collide with web routes / static assets.
pub const RESERVED_SLUGS: &[&str] = &[
    "vods", "api", "404", "500", "_astro", "_image", "_server-islands", "favicon.ico",
    "users", "admin", "static", "assets",
];

const SLUG_MAX: usize = 32;
const COMMAND_MAX: usize = 25;
const DISPLAY_NAME_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub slug: String,
    pub display_name: String,
    pub twitch_channel: String,
    pub idvk: String,
    pub enabled: bool,
    pub commands: Vec<String>,
    pub whitelist: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl User {
    pub fn to_draft(&self) -> UserDraft {
        UserDraft {
            slug: self.slug.clone(),
            display_name: self.display_name.clone(),
            twitch_channel: self.twitch_channel.clone(),
            idvk: self.idvk.clone(),
            enabled: self.enabled,
            commands: self.commands.clone(),
            whitelist: self.whitelist.clone(),
        }
    }

    /// Same data, ignoring timestamps (used to diff snapshots).
    pub fn same_content(&self, other: &User) -> bool {
        self.to_draft() == other.to_draft()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    DisplayName,
    Slug,
    TwitchChannel,
    Idvk,
    Commands,
    Whitelist,
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Field::DisplayName => "nombre",
            Field::Slug => "slug",
            Field::TwitchChannel => "canal twitch",
            Field::Idvk => "id vk",
            Field::Commands => "comandos",
            Field::Whitelist => "whitelist",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: Field,
    pub message: String,
}

impl FieldError {
    fn new(field: Field, message: impl Into<String>) -> Self {
        Self { field, message: message.into() }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Editable user data (no id / timestamps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDraft {
    pub slug: String,
    pub display_name: String,
    pub twitch_channel: String,
    pub idvk: String,
    pub enabled: bool,
    pub commands: Vec<String>,
    pub whitelist: Vec<String>,
}

impl Default for UserDraft {
    fn default() -> Self {
        Self {
            slug: String::new(),
            display_name: String::new(),
            twitch_channel: String::new(),
            idvk: String::new(),
            enabled: true,
            commands: vec!["#vk".into()],
            whitelist: Vec::new(),
        }
    }
}

impl UserDraft {
    /// Canonical form: trimmed, lowercase ids, no `#`/`@` prefixes on logins, deduped lists.
    pub fn normalized(&self) -> UserDraft {
        UserDraft {
            slug: self.slug.trim().to_lowercase(),
            display_name: self.display_name.trim().to_string(),
            twitch_channel: normalize_login(&self.twitch_channel.replace('#', "")),
            idvk: self.idvk.trim().to_string(),
            enabled: self.enabled,
            commands: dedupe(self.commands.iter().map(|c| clean_command(c))),
            whitelist: dedupe(self.whitelist.iter().map(|l| normalize_login(l))),
        }
    }

    /// Validates the normalized draft. Empty vec = OK.
    pub fn validate(&self) -> Vec<FieldError> {
        let d = self.normalized();
        let mut errors = Vec::new();

        if d.display_name.is_empty() {
            errors.push(FieldError::new(Field::DisplayName, "requerido"));
        } else if d.display_name.chars().count() > DISPLAY_NAME_MAX {
            errors.push(FieldError::new(
                Field::DisplayName,
                format!("máximo {DISPLAY_NAME_MAX} caracteres"),
            ));
        }

        if let Some(msg) = slug_error(&d.slug) {
            errors.push(FieldError::new(Field::Slug, msg));
        }

        if d.twitch_channel.is_empty() {
            errors.push(FieldError::new(Field::TwitchChannel, "requerido"));
        } else if !is_twitch_login(&d.twitch_channel) {
            errors.push(FieldError::new(
                Field::TwitchChannel,
                "login inválido (3-25: a-z 0-9 _)",
            ));
        }

        if d.idvk.is_empty() {
            errors.push(FieldError::new(Field::Idvk, "requerido (ej. id1117440596)"));
        } else if d.idvk.chars().any(char::is_whitespace) {
            errors.push(FieldError::new(Field::Idvk, "sin espacios"));
        }

        if d.commands.is_empty() {
            errors.push(FieldError::new(Field::Commands, "al menos un alias (ej. #vk)"));
        }
        for c in &d.commands {
            if c.chars().count() > COMMAND_MAX {
                errors.push(FieldError::new(
                    Field::Commands,
                    format!("'{c}' supera {COMMAND_MAX} caracteres"),
                ));
            }
        }

        for login in &d.whitelist {
            if !is_twitch_login(login) {
                errors.push(FieldError::new(
                    Field::Whitelist,
                    format!("'{login}' no es un login de Twitch válido"),
                ));
            }
        }

        errors
    }
}

fn slug_error(slug: &str) -> Option<String> {
    if slug.is_empty() {
        return Some("requerido".into());
    }
    if slug.len() > SLUG_MAX {
        return Some(format!("máximo {SLUG_MAX} caracteres"));
    }
    let mut chars = slug.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !first_ok || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
        return Some("solo a-z 0-9 _ - (empieza con letra o número)".into());
    }
    if RESERVED_SLUGS.contains(&slug) {
        return Some(format!("'{slug}' está reservado"));
    }
    None
}

fn is_twitch_login(login: &str) -> bool {
    (3..=25).contains(&login.len())
        && login
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn normalize_login(raw: &str) -> String {
    raw.trim().trim_start_matches('@').to_lowercase()
}

fn dedupe(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.to_lowercase()))
        .collect()
}

/// Invisible / format chars that Twitch clients sometimes append (e.g. U+034F).
pub fn is_invisible_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}' // soft hyphen
                | '\u{034F}' // combining grapheme joiner — seen in chat as `#vk ͏`
                | '\u{061C}' // arabic letter mark
                | '\u{180E}' // mongolian vowel separator
                | '\u{200B}'..='\u{200F}' // zwsp, zwnj, zwj, lrm, rlm
                | '\u{202A}'..='\u{202E}' // bidi overrides
                | '\u{2060}'..='\u{2064}' // word joiner, etc.
                | '\u{2066}'..='\u{206F}'
                | '\u{FEFF}' // bom / zwnbsp
                | '\u{E0000}'..='\u{E007F}' // tags
        )
}

/// Lowercase, without whitespace and invisible junk. Used for both storing aliases and
/// matching chat messages, so `#VK ͏` matches alias `#vk`.
pub fn clean_command(raw: &str) -> String {
    raw.chars()
        .filter(|&c| !c.is_whitespace() && !is_invisible_char(c))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// `"a, b ,,c"` → `["a", "b", "c"]`
pub fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> UserDraft {
        UserDraft {
            slug: "thedarkraimola".into(),
            display_name: "SapoPerro Ruso".into(),
            twitch_channel: "#TheDarkraiMola".into(),
            idvk: "id1117440596".into(),
            enabled: true,
            commands: vec!["#VK".into(), "#vk".into(), "!web".into()],
            whitelist: vec!["@User1".into(), "user1".into()],
        }
    }

    #[test]
    fn normalizes() {
        let d = draft().normalized();
        assert_eq!(d.twitch_channel, "thedarkraimola");
        assert_eq!(d.commands, vec!["#vk", "!web"]);
        assert_eq!(d.whitelist, vec!["user1"]);
        assert!(draft().validate().is_empty());
    }

    #[test]
    fn rejects_bad_fields() {
        let mut d = draft();
        d.slug = "vods".into();
        d.twitch_channel = "a b".into();
        d.commands.clear();
        d.whitelist = vec!["no-dash".into()];
        let fields: Vec<Field> = d.validate().into_iter().map(|e| e.field).collect();
        assert!(fields.contains(&Field::Slug));
        assert!(fields.contains(&Field::TwitchChannel));
        assert!(fields.contains(&Field::Commands));
        assert!(fields.contains(&Field::Whitelist));
    }

    #[test]
    fn slug_rules() {
        assert!(slug_error("otro_canal-2").is_none());
        assert!(slug_error("-x").is_some());
        assert!(slug_error("Mayus").is_some());
        assert!(slug_error("api").is_some());
    }

    #[test]
    fn cleans_commands_and_csv() {
        assert_eq!(clean_command(" #VK \u{034F}"), "#vk");
        assert_eq!(parse_csv("a, b ,,c "), vec!["a", "b", "c"]);
    }
}
