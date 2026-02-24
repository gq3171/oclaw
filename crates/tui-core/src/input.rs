//! Input mode definitions for TUI.

/// Input mode for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum InputMode {
    #[default]
    Normal,
    Insert,
    Command,
}
