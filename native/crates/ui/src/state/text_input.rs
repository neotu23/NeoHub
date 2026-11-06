//! A minimal, dependency-free text field model.
//!
//! The styled input widget from the component library needs its own state entity and focus
//! plumbing; for the sidebar filter (and the small single-line editors) a plain model is
//! enough, and it is far easier to test: every keystroke rule lives in [`apply_key`] and is
//! covered by unit tests instead of by clicking.

/// Result of one key press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The field absorbed the key (text changed).
    Changed,
    /// The key was not text (e.g. an arrow) — the caller may still want to handle it.
    Ignored,
    /// The field should lose focus / the caller should close the editor.
    Escape,
    /// The caller should submit the current value.
    Submit,
}

/// Mutable single-line text field state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    /// Caret position in bytes (always on a char boundary).
    caret: usize,
}

impl TextField {
    pub fn new(initial: impl Into<String>) -> Self {
        let text: String = initial.into();
        let caret = text.len();
        TextField { text, caret }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    /// Insert at the caret.
    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
    }

    /// Delete the character before the caret (never splits a UTF-8 character).
    pub fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        let previous = self.text[..self.caret]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.text.replace_range(previous..self.caret, "");
        self.caret = previous;
        true
    }

    /// Delete the character after the caret.
    pub fn delete(&mut self) -> bool {
        if self.caret >= self.text.len() {
            return false;
        }
        let next = self.text[self.caret..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.caret + index)
            .unwrap_or(self.text.len());
        self.text.replace_range(self.caret..next, "");
        true
    }

    pub fn move_left(&mut self) {
        if let Some((index, _)) = self.text[..self.caret].char_indices().last() {
            self.caret = index;
        }
    }

    pub fn move_right(&mut self) {
        if let Some((index, _)) = self.text[self.caret..].char_indices().nth(1) {
            self.caret += index;
        } else {
            self.caret = self.text.len();
        }
    }

    pub fn move_home(&mut self) {
        self.caret = 0;
    }

    pub fn move_end(&mut self) {
        self.caret = self.text.len();
    }

    /// Apply one keystroke.
    ///
    /// `key` is GPUI's `Keystroke::key` (`"a"`, `"backspace"`, `"enter"`, `"escape"`, …);
    /// `characters` is the composed text for printable input (already IME-resolved).
    pub fn apply_key(&mut self, key: &str, characters: Option<&str>) -> KeyOutcome {
        match key {
            "backspace" => {
                if self.backspace() {
                    KeyOutcome::Changed
                } else {
                    KeyOutcome::Ignored
                }
            }
            "delete" => {
                if self.delete() {
                    KeyOutcome::Changed
                } else {
                    KeyOutcome::Ignored
                }
            }
            "left" => {
                self.move_left();
                KeyOutcome::Ignored
            }
            "right" => {
                self.move_right();
                KeyOutcome::Ignored
            }
            "home" => {
                self.move_home();
                KeyOutcome::Ignored
            }
            "end" => {
                self.move_end();
                KeyOutcome::Ignored
            }
            "escape" => KeyOutcome::Escape,
            "enter" => KeyOutcome::Submit,
            _ => match characters {
                Some(text) if !text.is_empty() && !text.chars().any(char::is_control) => {
                    self.insert(text);
                    KeyOutcome::Changed
                }
                _ => KeyOutcome::Ignored,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut field = TextField::new("");
        assert_eq!(field.apply_key("d", Some("d")), KeyOutcome::Changed);
        field.apply_key("e", Some("e"));
        field.apply_key("m", Some("m"));
        assert_eq!(field.text(), "dem");
        field.move_left();
        field.apply_key("o", Some("o"));
        assert_eq!(field.text(), "deom");
        assert_eq!(field.caret(), 3);
    }

    #[test]
    fn backspace_and_delete_respect_utf8_boundaries() {
        let mut field = TextField::new("中文abc");
        // Caret at the end: backspace removes the ASCII characters, then a whole CJK char.
        assert!(field.backspace());
        assert_eq!(field.text(), "中文ab");
        field.move_home();
        assert!(field.delete());
        assert_eq!(field.text(), "文ab");
        field.move_end();
        field.backspace();
        assert_eq!(field.text(), "文a");
        field.backspace();
        field.backspace();
        assert_eq!(field.text(), "");
        assert!(!field.backspace(), "backspace at the start is a no-op");
    }

    #[test]
    fn quotes_and_chinese_input_are_accepted() {
        let mut field = TextField::new("");
        field.apply_key("中", Some("中"));
        field.apply_key("键", Some("键"));
        field.apply_key(" ", Some(" "));
        field.apply_key("-", Some("-"));
        assert_eq!(field.text(), "中键 -");
    }

    #[test]
    fn control_keys_are_classified() {
        let mut field = TextField::new("abc");
        assert_eq!(field.apply_key("escape", None), KeyOutcome::Escape);
        assert_eq!(field.apply_key("enter", None), KeyOutcome::Submit);
        assert_eq!(field.apply_key("left", None), KeyOutcome::Ignored);
        assert_eq!(field.caret(), 2);
        assert_eq!(field.apply_key("right", None), KeyOutcome::Ignored);
        assert_eq!(field.caret(), 3);
        assert_eq!(field.apply_key("home", None), KeyOutcome::Ignored);
        assert_eq!(field.caret(), 0);
        assert_eq!(field.apply_key("end", None), KeyOutcome::Ignored);
        assert_eq!(field.caret(), 3);
    }

    #[test]
    fn control_characters_never_enter_the_buffer() {
        let mut field = TextField::new("");
        assert_eq!(field.apply_key("x", Some("\u{7}")), KeyOutcome::Ignored);
        assert_eq!(field.text(), "");
        assert_eq!(field.apply_key("x", Some("")), KeyOutcome::Ignored);
    }

    #[test]
    fn set_and_clear_keep_the_caret_consistent() {
        let mut field = TextField::new("abc");
        field.set_text("hello");
        assert_eq!(field.text(), "hello");
        assert_eq!(field.caret(), 5);
        field.clear();
        assert_eq!(field.text(), "");
        assert_eq!(field.caret(), 0);
    }
}
