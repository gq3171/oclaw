use crate::theme::*;
/// Stdout rendering utilities for the lightweight terminal mode.
use std::io::{self, Write};

/// Print the startup header box.
pub fn print_header(model: &str, session: &str) {
    let version = env!("CARGO_PKG_VERSION");
    let line1 = format!("  OpenClaw v{}", version);
    let line2 = format!("  Model: {} · Session: {}", model, session);
    let width = line1.len().max(line2.len()) + 4;
    let bar = "─".repeat(width);

    println!("{CYAN}╭{bar}╮{RESET}");
    println!(
        "{CYAN}│{RESET}{BOLD_WHITE}{:<w$}{RESET}  {CYAN}│{RESET}",
        line1,
        w = width
    );
    println!("{CYAN}│{RESET}{:<w$}  {CYAN}│{RESET}", line2, w = width);
    println!("{CYAN}╰{bar}╯{RESET}");
    println!();
}

/// Print a thin separator line between messages.
pub fn print_separator() {
    println!("{GRAY}{}{RESET}", "─".repeat(40));
}

/// Print a system/info message.
pub fn print_system(text: &str) {
    println!("{CYAN}  ℹ {text}{RESET}");
}

/// Print a warning message.
pub fn print_warning(text: &str) {
    println!("{YELLOW}  ⚠ {text}{RESET}");
}

/// Print an error message.
pub fn print_error(text: &str) {
    println!("{RED}  ✗ {text}{RESET}");
}

/// Print a tool call status line.
pub fn print_tool_running(name: &str) {
    print!("{CYAN}  ⟳ {name}...{RESET}");
    io::stdout().flush().ok();
}

/// Print tool completion (overwrites the running line).
pub fn print_tool_done(name: &str, success: bool) {
    if success {
        println!("\r{GREEN}  ✓ {name}{RESET}      ");
    } else {
        println!("\r{RED}  ✗ {name}{RESET}      ");
    }
}

/// Print a streaming text chunk (no newline, flush immediately).
pub fn print_chunk(text: &str) {
    print!("{}", text);
    io::stdout().flush().ok();
}

/// Finish streaming output (ensure newline).
pub fn finish_stream() {
    println!();
}

/// Print the user's input echo.
pub fn print_user_message(text: &str) {
    println!("{BOLD_GREEN}> {RESET}{}", text);
}

/// Print connection status.
pub fn print_connected(url: &str) {
    println!("{GREEN}  ● Connected to {url}{RESET}");
}

pub fn print_disconnected(url: &str) {
    println!("{RED}  ○ Failed to connect to {url}{RESET}");
}

/// Print a history message (dimmed, for loaded transcript).
pub fn print_history_message(role: &str, content: &str) {
    let truncated = if content.len() > 200 {
        // Find a char boundary at or before byte 200
        let mut end = 200;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &content[..end])
    } else {
        content.to_string()
    };
    match role {
        "user" => println!("{DIM}{GREEN}  > {}{RESET}", truncated),
        "assistant" => println!("{DIM}  {}{RESET}", truncated),
        _ => {}
    }
}

/// Print help text.
pub fn print_help() {
    println!("{BOLD_WHITE}Commands:{RESET}");
    println!("  {CYAN}/help{RESET}      Show this help");
    println!("  {CYAN}/clear{RESET}     Clear screen");
    println!("  {CYAN}/model{RESET}     Switch model");
    println!("  {CYAN}/session{RESET}   Switch session");
    println!("  {CYAN}/status{RESET}    Show connection info");
    println!("  {CYAN}/verbose{RESET}   Toggle verbose mode");
    println!("  {CYAN}/exit{RESET}      Quit");
}
