//! ANSI color constants for terminal output.

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";

// Foreground colors
pub const CYAN: &str = "\x1b[36m";
pub const GREEN: &str = "\x1b[32m";
pub const RED: &str = "\x1b[31m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const WHITE: &str = "\x1b[37m";
pub const GRAY: &str = "\x1b[90m";

pub const BOLD_CYAN: &str = "\x1b[1;36m";
pub const BOLD_GREEN: &str = "\x1b[1;32m";
pub const BOLD_RED: &str = "\x1b[1;31m";
pub const BOLD_YELLOW: &str = "\x1b[1;33m";
pub const BOLD_WHITE: &str = "\x1b[1;37m";
