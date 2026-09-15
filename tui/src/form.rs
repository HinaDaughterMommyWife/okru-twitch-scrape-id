//! Create / edit form state: inputs, focus, live validation.

use okru_tui::db::{parse_csv, Field, FieldError, User, UserDraft};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::TextInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    DisplayName,
    TwitchChannel,
    Slug,
    Idvk,
    Commands,
    Whitelist,
    Enabled,
}

impl FormField {
    pub const ALL: [FormField; 7] = [
        FormField::DisplayName,
        FormField::TwitchChannel,
        FormField::Slug,
        FormField::Idvk,
        FormField::Commands,
        FormField::Whitelist,
        FormField::Enabled,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FormField::DisplayName => "Nombre visible",
            FormField::TwitchChannel => "Canal Twitch",
            FormField::Slug => "Slug (URL)",
            FormField::Idvk => "ID VK",
            FormField::Commands => "Comandos",
            FormField::Whitelist => "Whitelist",
            FormField::Enabled => "Activo",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            FormField::DisplayName => "título de la página, ej. SapoPerro Ruso",
            FormField::TwitchChannel => "login del canal, sin #",
            FormField::Slug => "watch.shonensemanal.site/<slug> · se autocompleta desde el canal",
            FormField::Idvk => "perfil/comunidad VK, ej. id1117440596",
            FormField::Commands => "alias separados por coma: #vk, #okru, !web",
            FormField::Whitelist => "logins que también pueden usar el comando: user1, user2",
            FormField::Enabled => "espacio para alternar · desactivado = el bot sale del canal",
        }
    }

    pub fn is_list(self) -> bool {
        matches!(self, FormField::Commands | FormField::Whitelist)
    }

    fn db_field(self) -> Option<Field> {
        match self {
            FormField::DisplayName => Some(Field::DisplayName),
            FormField::TwitchChannel => Some(Field::TwitchChannel),
            FormField::Slug => Some(Field::Slug),
            FormField::Idvk => Some(Field::Idvk),
            FormField::Commands => Some(Field::Commands),
            FormField::Whitelist => Some(Field::Whitelist),
            FormField::Enabled => None,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap()
    }
}

pub enum FormOutcome {
    Continue,
    Save,
    Cancel,
}

pub struct Form {
    /// `None` = creating a new user.
    pub editing_id: Option<i64>,
    inputs: [TextInput; 6],
    pub enabled: bool,
    pub focus: FormField,
    initial: UserDraft,
    errors: Vec<FieldError>,
    touched: [bool; 7],
    show_all_errors: bool,
    slug_linked: bool,
    /// Esc pressed with unsaved changes: waiting for a second Esc.
    pub confirm_discard: bool,
}

impl Form {
    pub fn create() -> Self {
        Self::from_draft(None, UserDraft::default(), true)
    }

    pub fn edit(user: &User) -> Self {
        Self::from_draft(Some(user.id), user.to_draft(), false)
    }

    /// New user prefilled from an existing one (slug/channel cleared).
    pub fn clone_from(user: &User) -> Self {
        let mut draft = user.to_draft();
        draft.slug.clear();
        draft.twitch_channel.clear();
        draft.display_name.clear();
        let mut form = Self::from_draft(None, draft, true);
        form.initial = UserDraft::default();
        form
    }

    fn from_draft(editing_id: Option<i64>, draft: UserDraft, slug_linked: bool) -> Self {
        let inputs = [
            TextInput::new(&draft.display_name),
            TextInput::new(&draft.twitch_channel),
            TextInput::new(&draft.slug),
            TextInput::new(&draft.idvk),
            TextInput::new(draft.commands.join(", ")),
            TextInput::new(draft.whitelist.join(", ")),
        ];
        let mut form = Self {
            editing_id,
            inputs,
            enabled: draft.enabled,
            focus: FormField::DisplayName,
            initial: draft,
            errors: Vec::new(),
            touched: [false; 7],
            show_all_errors: false,
            slug_linked,
            confirm_discard: false,
        };
        form.revalidate();
        form
    }

    pub fn title(&self) -> &'static str {
        if self.editing_id.is_some() {
            " Editar usuario "
        } else {
            " Nuevo usuario "
        }
    }

    pub fn input(&self, field: FormField) -> Option<&TextInput> {
        self.inputs.get(field.index())
    }

    pub fn draft(&self) -> UserDraft {
        let v = |f: FormField| self.inputs[f.index()].value().to_string();
        UserDraft {
            display_name: v(FormField::DisplayName),
            twitch_channel: v(FormField::TwitchChannel),
            slug: v(FormField::Slug),
            idvk: v(FormField::Idvk),
            enabled: self.enabled,
            commands: parse_csv(&v(FormField::Commands)),
            whitelist: parse_csv(&v(FormField::Whitelist)),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.draft().normalized() != self.initial.normalized()
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Visible error for a field (after it was touched or a save was attempted).
    pub fn error_for(&self, field: FormField) -> Option<String> {
        if !(self.show_all_errors || self.touched[field.index()]) {
            return None;
        }
        let db_field = field.db_field()?;
        let messages: Vec<&str> = self
            .errors
            .iter()
            .filter(|e| e.field == db_field)
            .map(|e| e.message.as_str())
            .collect();
        (!messages.is_empty()).then(|| messages.join(" · "))
    }

    /// Normalized preview of list fields (chips).
    pub fn chips(&self, field: FormField) -> Vec<String> {
        let d = self.draft().normalized();
        match field {
            FormField::Commands => d.commands,
            FormField::Whitelist => d.whitelist,
            _ => Vec::new(),
        }
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Save was attempted: reveal every error.
    pub fn reveal_errors(&mut self) {
        self.show_all_errors = true;
    }

    /// Server-side error (e.g. UNIQUE conflict) attached to a field.
    pub fn push_error(&mut self, field: Field, message: impl Into<String>) {
        self.errors.push(FieldError { field, message: message.into() });
        self.show_all_errors = true;
        if let Some(f) = FormField::ALL.iter().find(|f| f.db_field() == Some(field)) {
            self.focus = *f;
        }
    }

    fn revalidate(&mut self) {
        self.errors = self.draft().validate();
    }

    fn move_focus(&mut self, delta: isize) {
        let all = FormField::ALL;
        let i = self.focus.index() as isize + delta;
        let i = i.rem_euclid(all.len() as isize) as usize;
        self.touched[self.focus.index()] = true;
        self.focus = all[i];
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FormOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if key.code != KeyCode::Esc {
            self.confirm_discard = false;
        }

        match key.code {
            KeyCode::Char('s') if ctrl => return FormOutcome::Save,
            KeyCode::Esc => {
                if !self.is_dirty() || self.confirm_discard {
                    return FormOutcome::Cancel;
                }
                self.confirm_discard = true;
                return FormOutcome::Continue;
            }
            KeyCode::Tab | KeyCode::Down => self.move_focus(1),
            KeyCode::BackTab | KeyCode::Up => self.move_focus(-1),
            KeyCode::Enter => {
                if self.focus == FormField::Enabled {
                    return FormOutcome::Save;
                }
                self.move_focus(1);
            }
            _ if self.focus == FormField::Enabled => {
                if matches!(key.code, KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right) {
                    self.enabled = !self.enabled;
                }
            }
            _ => {
                let idx = self.focus.index();
                if self.inputs[idx].handle(key) {
                    self.touched[idx] = true;
                    self.on_value_changed();
                }
            }
        }
        self.revalidate();
        FormOutcome::Continue
    }

    fn on_value_changed(&mut self) {
        match self.focus {
            FormField::Slug => self.slug_linked = self.inputs[FormField::Slug.index()].value().is_empty(),
            FormField::TwitchChannel if self.slug_linked => {
                let channel = self.inputs[FormField::TwitchChannel.index()]
                    .value()
                    .trim()
                    .trim_start_matches('#')
                    .to_lowercase();
                self.inputs[FormField::Slug.index()].set(channel);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(form: &mut Form, s: &str) {
        for c in s.chars() {
            form.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    fn tab(form: &mut Form) {
        form.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    }

    #[test]
    fn slug_follows_channel_until_edited() {
        let mut form = Form::create();
        type_str(&mut form, "Otro");
        tab(&mut form);
        type_str(&mut form, "#OtroCanal");
        assert_eq!(form.input(FormField::Slug).unwrap().value(), "otrocanal");

        tab(&mut form);
        type_str(&mut form, "x");
        assert_eq!(form.input(FormField::Slug).unwrap().value(), "otrocanalx");

        // Slug was edited by hand: channel changes no longer overwrite it.
        form.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        type_str(&mut form, "2");
        assert_eq!(form.input(FormField::Slug).unwrap().value(), "otrocanalx");
        assert!(form.is_dirty());
    }

    #[test]
    fn errors_hidden_until_touched_or_saved() {
        let mut form = Form::create();
        assert!(form.has_errors());
        assert!(form.error_for(FormField::Idvk).is_none());
        form.reveal_errors();
        assert!(form.error_for(FormField::Idvk).is_some());
    }

    #[test]
    fn esc_requires_confirmation_when_dirty() {
        let mut form = Form::create();
        type_str(&mut form, "x");
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(form.handle_key(esc), FormOutcome::Continue));
        assert!(matches!(form.handle_key(esc), FormOutcome::Cancel));
    }
}
