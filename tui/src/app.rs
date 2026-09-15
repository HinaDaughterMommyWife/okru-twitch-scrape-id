//! Application state + key handling (rendering lives in `ui.rs`).

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use okru_tui::config::SharedConfig;
use okru_tui::db::{Store, StoreError, User};
use okru_tui::ipc::{CheckOutcome, ServerMsg};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;

use crate::form::{Form, FormOutcome};
use crate::input::TextInput;
use crate::link::{Link, LinkEvent};

const TOAST_TTL: Duration = Duration::from_secs(4);
/// In-memory only (no log file): oldest lines are dropped.
pub const ACTIVITY_MAX: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Ok,
    Info,
    Error,
}

pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
    at: Instant,
}

/// One line of bot activity received over the IPC link.
pub struct Activity {
    pub kind: ToastKind,
    pub text: String,
    pub at: Instant,
}

pub enum Mode {
    List,
    Filter,
    Form(Box<Form>),
    ConfirmDelete { id: i64, slug: String, input: TextInput },
    Help,
    /// Focus on the bot activity panel (scrollable).
    Activity,
}

pub struct App {
    store: Store,
    pub db_path: PathBuf,
    /// `webURL` / `ipcPort` from config.toml.
    pub config: SharedConfig,
    pub users: Vec<User>,
    pub filter: TextInput,
    pub table: TableState,
    pub mode: Mode,
    pub toast: Option<Toast>,
    pub should_quit: bool,
    pub link: Link,
    /// Channels Twitch confirmed the bot is in (from welcome / JOIN / PART).
    pub bot_channels: BTreeSet<String>,
    /// Newest first, at most [`ACTIVITY_MAX`].
    pub activity: VecDeque<Activity>,
    /// Lines scrolled away from the newest entry (0 = following new events).
    pub activity_scroll: usize,
    /// Visible activity rows, updated on every draw (for paging / clamping).
    pub activity_page: usize,
}

impl App {
    pub fn new(store: Store, db_path: PathBuf, config: SharedConfig, link: Link) -> anyhow::Result<Self> {
        let mut app = Self {
            users: store.list()?,
            store,
            db_path,
            config,
            filter: TextInput::default(),
            table: TableState::default(),
            mode: Mode::List,
            toast: None,
            should_quit: false,
            link,
            bot_channels: BTreeSet::new(),
            activity: VecDeque::new(),
            activity_scroll: 0,
            activity_page: 1,
        };
        app.clamp_selection();
        Ok(app)
    }

    // ---------- queries ----------

    pub fn visible(&self) -> Vec<&User> {
        let q = self.filter.value().trim().to_lowercase();
        self.users
            .iter()
            .filter(|u| {
                q.is_empty()
                    || u.slug.contains(&q)
                    || u.twitch_channel.contains(&q)
                    || u.display_name.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn selected(&self) -> Option<&User> {
        let idx = self.table.selected()?;
        self.visible().get(idx).copied()
    }

    pub fn active_count(&self) -> usize {
        self.users.iter().filter(|u| u.enabled).count()
    }

    // ---------- lifecycle ----------

    /// Whether Twitch confirmed the bot is in this user's channel (`None` = backend offline).
    pub fn bot_in_channel(&self, user: &User) -> Option<bool> {
        self.link
            .is_connected()
            .then(|| self.bot_channels.contains(&user.twitch_channel))
    }

    /// Periodic work: expire toasts, process backend events.
    pub fn on_tick(&mut self) {
        if self.toast.as_ref().is_some_and(|t| t.at.elapsed() > TOAST_TTL) {
            self.toast = None;
        }
        for event in self.link.poll() {
            self.on_link_event(event);
        }
    }

    fn on_link_event(&mut self, event: LinkEvent) {
        match event {
            LinkEvent::Connected => {
                let port = self.link.port;
                self.log(ToastKind::Ok, format!("conectado al bot (127.0.0.1:{port})"));
            }
            LinkEvent::Disconnected => {
                self.bot_channels.clear();
                self.log(ToastKind::Error, "bot desconectado · reintentando…");
            }
            LinkEvent::Message(msg) => self.on_server_msg(msg),
        }
    }

    fn on_server_msg(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Welcome { active, channels, .. } => {
                self.bot_channels = channels.into_iter().collect();
                self.log(ToastKind::Info, format!("bot con {active} usuarios activos"));
            }
            ServerMsg::Reloaded { active, by, .. } => {
                let ours = by.as_deref() == Some(self.link.client_id.as_str());
                if !ours {
                    self.reload(None);
                    self.notify(ToastKind::Info, "↻ Cambios de otro TUI recargados");
                }
                self.log(ToastKind::Info, format!("bot recargó la DB ({active} activos)"));
            }
            ServerMsg::Joined { channel } => {
                self.notify(ToastKind::Ok, format!("🤖 el bot entró a #{channel}"));
                self.log(ToastKind::Ok, format!("JOIN #{channel}"));
                self.bot_channels.insert(channel);
            }
            ServerMsg::Parted { channel } => {
                self.log(ToastKind::Info, format!("PART #{channel}"));
                self.bot_channels.remove(&channel);
            }
            ServerMsg::Synced { slug, action } => {
                let what = if action == "delete" { "página retirada" } else { "página publicada" };
                self.log(ToastKind::Ok, format!("web: {what} /{slug}"));
            }
            ServerMsg::SyncFailed { slug, error } => {
                self.notify(ToastKind::Error, format!("✗ worker /{slug}: reintentando"));
                self.log(ToastKind::Error, format!("web /{slug}: {error}"));
            }
            ServerMsg::CommandUsed { channel, user, role, command, accepted, .. } => {
                let who = format!("#{channel} · {user} ({})", role.label());
                if accepted {
                    self.notify(ToastKind::Info, format!("💬 {user} usó {command} en #{channel}"));
                    self.log(ToastKind::Info, format!("{who} usó {command}"));
                } else {
                    self.log(ToastKind::Info, format!("{who} usó {command} — ignorado, búsqueda en curso"));
                }
            }
            ServerMsg::CheckResult { slug, outcome, detail } => match outcome {
                CheckOutcome::Live => {
                    self.notify(ToastKind::Ok, format!("🔴 /{slug}: stream en vivo detectado"));
                    self.log(ToastKind::Ok, format!("/{slug}: stream en vivo detectado"));
                }
                CheckOutcome::Offline => self.log(ToastKind::Info, format!("/{slug}: sin stream activo")),
                CheckOutcome::Error => {
                    let detail = detail.unwrap_or_default();
                    self.log(ToastKind::Error, format!("/{slug}: error en la búsqueda {detail}"));
                }
                CheckOutcome::Timeout => {
                    self.log(ToastKind::Error, format!("/{slug}: la búsqueda no respondió (timeout)"))
                }
            },
        }
    }

    fn log(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.activity.push_front(Activity { kind, text: text.into(), at: Instant::now() });
        self.activity.truncate(ACTIVITY_MAX);
        // Reading older lines: keep them in place instead of jumping.
        if self.activity_scroll > 0 {
            self.activity_scroll += 1;
        }
        self.clamp_activity_scroll();
    }

    pub fn max_activity_scroll(&self) -> usize {
        self.activity.len().saturating_sub(self.activity_page.max(1))
    }

    fn clamp_activity_scroll(&mut self) {
        self.activity_scroll = self.activity_scroll.min(self.max_activity_scroll());
    }

    fn scroll_activity(&mut self, delta: isize) {
        let next = (self.activity_scroll as isize + delta).max(0) as usize;
        self.activity_scroll = next;
        self.clamp_activity_scroll();
    }

    /// After every committed write: tell the backend and report what happened.
    fn committed(&mut self, message: String) {
        if self.link.notify_changed() {
            self.notify(ToastKind::Ok, format!("{message} · bot avisado"));
        } else {
            self.notify(
                ToastKind::Info,
                format!("{message} · bot desconectado: se aplica al iniciarlo"),
            );
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::List) {
            Mode::List => self.on_list_key(key),
            Mode::Filter => self.on_filter_key(key),
            Mode::Form(form) => self.on_form_key(form, key),
            Mode::ConfirmDelete { id, slug, input } => self.on_delete_key(id, slug, input, key),
            Mode::Help => {} // any key closes help
            Mode::Activity => self.on_activity_key(key),
        }
    }

    fn notify(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.toast = Some(Toast { kind, message: message.into(), at: Instant::now() });
    }

    /// Reloads users; keeps selection on `select_id` or the currently selected user.
    fn reload(&mut self, select_id: Option<i64>) {
        let keep = select_id.or_else(|| self.selected().map(|u| u.id));
        match self.store.list() {
            Ok(users) => self.users = users,
            Err(e) => {
                self.notify(ToastKind::Error, format!("No se pudo leer la DB: {e}"));
                return;
            }
        }
        if let Some(id) = keep {
            if let Some(pos) = self.visible().iter().position(|u| u.id == id) {
                self.table.select(Some(pos));
            }
        }
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let len = self.visible().len();
        let sel = match (len, self.table.selected()) {
            (0, _) => None,
            (_, None) => Some(0),
            (n, Some(i)) => Some(i.min(n - 1)),
        };
        self.table.select(sel);
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.visible().len();
        if len == 0 {
            return;
        }
        let i = self.table.selected().unwrap_or(0) as isize + delta;
        self.table.select(Some(i.clamp(0, len as isize - 1) as usize));
    }

    // ---------- modes ----------

    fn on_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc if !self.filter.value().is_empty() => {
                self.filter.clear();
                self.clamp_selection();
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') | KeyCode::Home => self.table.select(Some(0)),
            KeyCode::Char('G') | KeyCode::End => self.move_selection(isize::MAX / 2),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Tab => self.mode = Mode::Activity,
            KeyCode::Char('a') | KeyCode::Char('n') => self.mode = Mode::Form(Box::new(Form::create())),
            KeyCode::Char('c') => {
                if let Some(user) = self.selected() {
                    self.mode = Mode::Form(Box::new(Form::clone_from(user)));
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(user) = self.selected() {
                    self.mode = Mode::Form(Box::new(Form::edit(user)));
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(user) = self.selected() {
                    self.mode = Mode::ConfirmDelete {
                        id: user.id,
                        slug: user.slug.clone(),
                        input: TextInput::default(),
                    };
                }
            }
            KeyCode::Char(' ') => self.toggle_selected(),
            KeyCode::Char('r') => {
                self.reload(None);
                self.notify(ToastKind::Info, "↻ Recargado");
            }
            _ => {}
        }
    }

    fn on_activity_key(&mut self, key: KeyEvent) {
        let page = self.activity_page.max(1) as isize;
        match key.code {
            KeyCode::Tab | KeyCode::Esc => return,
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => self.scroll_activity(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_activity(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_activity(page),
            KeyCode::PageUp => self.scroll_activity(-page),
            KeyCode::Char('g') | KeyCode::Home => self.activity_scroll = 0,
            KeyCode::Char('G') | KeyCode::End => self.activity_scroll = self.max_activity_scroll(),
            KeyCode::Char('x') => {
                self.activity.clear();
                self.activity_scroll = 0;
            }
            _ => {}
        }
        self.mode = Mode::Activity;
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.filter.clear(),
            KeyCode::Enter => {}
            KeyCode::Down | KeyCode::Up => {
                self.move_selection(if key.code == KeyCode::Down { 1 } else { -1 });
                self.mode = Mode::Filter;
            }
            _ => {
                self.filter.handle(key);
                self.table.select(Some(0));
                self.mode = Mode::Filter;
            }
        }
        self.clamp_selection();
    }

    fn on_form_key(&mut self, mut form: Box<Form>, key: KeyEvent) {
        match form.handle_key(key) {
            FormOutcome::Continue => self.mode = Mode::Form(form),
            FormOutcome::Cancel => {}
            FormOutcome::Save => self.save(form),
        }
    }

    fn save(&mut self, mut form: Box<Form>) {
        if form.has_errors() {
            form.reveal_errors();
            let n = form.error_count();
            self.notify(ToastKind::Error, format!("✗ Revisa {n} error(es) antes de guardar"));
            self.mode = Mode::Form(form);
            return;
        }

        let draft = form.draft();
        let result = match form.editing_id {
            Some(id) => self.store.update(id, &draft).map(|_| id),
            None => self.store.insert(&draft),
        };

        match result {
            Ok(id) => {
                let verb = if form.editing_id.is_some() { "actualizado" } else { "creado" };
                self.filter.clear();
                self.reload(Some(id));
                self.committed(format!("✓ '{}' {verb}", draft.normalized().slug));
            }
            Err(StoreError::Conflict(field)) => {
                form.push_error(field, "ya está en uso por otro usuario");
                self.notify(ToastKind::Error, format!("✗ {} duplicado", field));
                self.mode = Mode::Form(form);
            }
            Err(StoreError::Invalid(errors)) => {
                for e in errors {
                    form.push_error(e.field, e.message);
                }
                self.mode = Mode::Form(form);
            }
            Err(e) => {
                self.notify(ToastKind::Error, format!("✗ {e}"));
                self.mode = Mode::Form(form);
            }
        }
    }

    fn on_delete_key(&mut self, id: i64, slug: String, mut input: TextInput, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter if input.value().trim() == slug => match self.store.delete(id) {
                Ok(()) => {
                    self.reload(None);
                    self.committed(format!("✓ '{slug}' eliminado"));
                }
                Err(e) => self.notify(ToastKind::Error, format!("✗ {e}")),
            },
            _ => {
                input.handle(key);
                self.mode = Mode::ConfirmDelete { id, slug, input };
            }
        }
    }

    fn toggle_selected(&mut self) {
        let Some((id, slug, enabled)) = self.selected().map(|u| (u.id, u.slug.clone(), u.enabled)) else {
            return;
        };
        match self.store.set_enabled(id, !enabled) {
            Ok(()) => {
                self.reload(Some(id));
                let state = if enabled { "○ desactivado" } else { "● activado" };
                self.committed(format!("'{slug}' {state}"));
            }
            Err(e) => self.notify(ToastKind::Error, format!("✗ {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use okru_tui::db::UserDraft;

    fn app_with(slugs: &[&str]) -> App {
        let mut store = Store::open_in_memory().unwrap();
        for slug in slugs {
            store
                .insert(&UserDraft {
                    slug: slug.to_string(),
                    display_name: slug.to_uppercase(),
                    twitch_channel: format!("chan_{slug}"),
                    idvk: "id1".into(),
                    ..UserDraft::default()
                })
                .unwrap();
        }
        App::new(store, PathBuf::from(":memory:"), SharedConfig::default(), Link::offline()).unwrap()
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn activity_scrolls_keeps_position_and_caps_at_100() {
        let mut app = app_with(&["uno"]);
        app.activity_page = 5;
        for i in 0..120 {
            app.log(ToastKind::Info, format!("evento {i}"));
        }
        assert_eq!(app.activity.len(), ACTIVITY_MAX);
        assert_eq!(app.activity.front().unwrap().text, "evento 119");

        press(&mut app, KeyCode::Tab);
        assert!(matches!(app.mode, Mode::Activity));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.activity_scroll, 6);

        // New event while reading older lines: the view does not jump.
        let visible_before = app.activity[app.activity_scroll].text.clone();
        app.on_server_msg(ServerMsg::CommandUsed {
            slug: "uno".into(),
            channel: "chan_uno".into(),
            user: "friend".into(),
            role: okru_tui::ipc::Role::Whitelist,
            command: "#okru".into(),
            accepted: true,
        });
        assert_eq!(app.activity[app.activity_scroll].text, visible_before);
        assert_eq!(app.activity.front().unwrap().text, "#chan_uno · friend (whitelist) usó #okru");

        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.activity_scroll, ACTIVITY_MAX - 5);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.activity_scroll, 0);
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.mode, Mode::List));
    }

    #[test]
    fn backend_events_update_presence_activity_and_reload() {
        let mut app = app_with(&["uno"]);
        app.on_server_msg(ServerMsg::Joined { channel: "chan_uno".into() });
        assert!(app.bot_channels.contains("chan_uno"));
        assert_eq!(app.activity.front().unwrap().text, "JOIN #chan_uno");

        // Another process added a user and told the backend: this TUI reloads.
        app.store
            .insert(&UserDraft {
                slug: "dos".into(),
                display_name: "Dos".into(),
                twitch_channel: "chan_dos".into(),
                idvk: "id1".into(),
                ..UserDraft::default()
            })
            .unwrap();
        app.on_server_msg(ServerMsg::Reloaded { users: 2, active: 2, by: Some("otro-tui".into()) });
        assert_eq!(app.users.len(), 2);

        // Our own reload is not re-applied.
        let own = app.link.client_id.clone();
        app.on_server_msg(ServerMsg::Reloaded { users: 2, active: 2, by: Some(own) });
        assert!(app.toast.as_ref().unwrap().message.contains("otro TUI"));

        app.on_server_msg(ServerMsg::Parted { channel: "chan_uno".into() });
        assert!(app.bot_channels.is_empty());
    }

    #[test]
    fn filter_toggle_and_delete_flow() {
        let mut app = app_with(&["uno", "dos", "tres"]);
        assert_eq!(app.visible().len(), 3);

        press(&mut app, KeyCode::Char('/'));
        for c in "tr".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.visible().len(), 1);
        assert_eq!(app.selected().unwrap().slug, "tres");

        press(&mut app, KeyCode::Char(' '));
        assert!(!app.selected().unwrap().enabled);

        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Enter); // wrong confirmation (empty) keeps modal
        assert!(matches!(app.mode, Mode::ConfirmDelete { .. }));
        for c in "tres".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::List));
        assert_eq!(app.users.len(), 2);
    }
}
