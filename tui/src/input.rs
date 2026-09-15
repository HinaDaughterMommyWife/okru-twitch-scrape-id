//! Single-line text input with a char-based cursor.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Span;

#[derive(Debug, Clone, Default)]
pub struct TextInput {
    value: String,
    /// Cursor position in chars (0..=len).
    cursor: usize,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set(&mut self, value: impl Into<String>) {
        *self = Self::new(value);
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    /// Terminal columns before the cursor (for `Frame::set_cursor_position`).
    pub fn cursor_col(&self) -> u16 {
        let prefix: String = self.value.chars().take(self.cursor).collect();
        Span::raw(prefix).width() as u16
    }

    /// Applies an editing key. Returns `true` if the value changed.
    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('w') if ctrl => self.delete_word_back(),
            KeyCode::Char('u') if ctrl => {
                let removed = self.cursor > 0;
                self.value = self.value.chars().skip(self.cursor).collect();
                self.cursor = 0;
                removed
            }
            KeyCode::Char('a') if ctrl => self.move_to(0),
            KeyCode::Char('e') if ctrl => self.move_to(self.len()),
            KeyCode::Char(c) if !ctrl => {
                let at = self.byte_index(self.cursor);
                self.value.insert(at, c);
                self.cursor += 1;
                true
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let at = self.byte_index(self.cursor);
                self.value.remove(at);
                true
            }
            KeyCode::Delete if self.cursor < self.len() => {
                let at = self.byte_index(self.cursor);
                self.value.remove(at);
                true
            }
            KeyCode::Left => self.move_to(self.cursor.saturating_sub(1)),
            KeyCode::Right => self.move_to((self.cursor + 1).min(self.len())),
            KeyCode::Home => self.move_to(0),
            KeyCode::End => self.move_to(self.len()),
            _ => false,
        }
    }

    fn len(&self) -> usize {
        self.value.chars().count()
    }

    fn move_to(&mut self, pos: usize) -> bool {
        self.cursor = pos;
        false
    }

    fn byte_index(&self, char_pos: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }

    fn delete_word_back(&mut self) -> bool {
        let chars: Vec<char> = self.value.chars().collect();
        let mut start = self.cursor;
        while start > 0 && (chars[start - 1].is_whitespace() || chars[start - 1] == ',') {
            start -= 1;
        }
        while start > 0 && !(chars[start - 1].is_whitespace() || chars[start - 1] == ',') {
            start -= 1;
        }
        if start == self.cursor {
            return false;
        }
        self.value = chars[..start].iter().chain(&chars[self.cursor..]).collect();
        self.cursor = start;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEvent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn edits_with_unicode_and_cursor() {
        let mut input = TextInput::new("añb");
        input.handle(key(KeyCode::Left));
        input.handle(key(KeyCode::Backspace));
        input.handle(key(KeyCode::Char('x')));
        assert_eq!(input.value(), "axb");
        input.handle(key(KeyCode::End));
        input.handle(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(input.value(), "");
    }

    #[test]
    fn ctrl_w_stops_at_commas() {
        let mut input = TextInput::new("#vk, #okru");
        input.handle(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(input.value(), "#vk, ");
    }
}
