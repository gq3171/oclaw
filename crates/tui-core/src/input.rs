//! Input handling for TUI

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Input mode for the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum InputMode {
    #[default]
    Normal,
    Insert,
    Command,
    Search,
}

/// Key binding definition
#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub key: String,
    pub action: super::AppEvent,
}

/// Input handler for the TUI
pub struct InputHandler {
    mode: InputMode,
    #[allow(dead_code)]
    bindings: Vec<KeyBinding>,
}

impl InputHandler {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        Self {
            mode: InputMode::Normal,
            bindings,
        }
    }

    pub fn set_mode(&mut self, mode: InputMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }

    pub fn handle_key(&self, key: KeyEvent) -> Option<super::AppEvent> {
        // Ctrl+C / Ctrl+Q always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => return Some(super::AppEvent::Quit),
                _ => {}
            }
        }

        match self.mode {
            InputMode::Normal => self.handle_normal(key),
            InputMode::Insert => self.handle_insert(key),
            InputMode::Command => self.handle_insert(key),
            InputMode::Search => self.handle_insert(key),
        }
    }

    fn handle_normal(&self, key: KeyEvent) -> Option<super::AppEvent> {
        match key.code {
            KeyCode::Char('q') => Some(super::AppEvent::Quit),
            KeyCode::Char('?') => Some(super::AppEvent::ToggleHelp),
            KeyCode::Char('r') => Some(super::AppEvent::Refresh),
            KeyCode::Char('/') => Some(super::AppEvent::Search),
            KeyCode::Tab => Some(super::AppEvent::NextScreen),
            KeyCode::BackTab => Some(super::AppEvent::PreviousScreen),
            KeyCode::Down | KeyCode::Char('j') => Some(super::AppEvent::NextItem),
            KeyCode::Up | KeyCode::Char('k') => Some(super::AppEvent::PreviousItem),
            KeyCode::Enter => Some(super::AppEvent::Select),
            KeyCode::Esc => Some(super::AppEvent::Cancel),
            KeyCode::Char('i') => Some(super::AppEvent::InsertChar('\0')), // switch to insert mode signal
            _ => None,
        }
    }

    fn handle_insert(&self, key: KeyEvent) -> Option<super::AppEvent> {
        match key.code {
            KeyCode::Esc => Some(super::AppEvent::Cancel),
            KeyCode::Enter => Some(super::AppEvent::Submit),
            KeyCode::Char(c) => Some(super::AppEvent::InsertChar(c)),
            _ => None,
        }
    }
}

pub fn default_keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding { key: "q".into(), action: super::AppEvent::Quit },
        KeyBinding { key: "?".into(), action: super::AppEvent::ToggleHelp },
        KeyBinding { key: "Tab".into(), action: super::AppEvent::NextScreen },
        KeyBinding { key: "j".into(), action: super::AppEvent::NextItem },
        KeyBinding { key: "k".into(), action: super::AppEvent::PreviousItem },
        KeyBinding { key: "Enter".into(), action: super::AppEvent::Select },
        KeyBinding { key: "r".into(), action: super::AppEvent::Refresh },
    ]
}
