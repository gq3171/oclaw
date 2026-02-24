/// Slash command parser and definitions for the TUI.

#[derive(Debug, Clone)]
pub enum SlashCommand {
    Help,
    Clear,
    Exit,
    Status,
    Model(Option<String>),
    Session(Option<String>),
    Think(Option<String>),
    Verbose,
    Usage,
    Abort,
}

#[derive(Debug)]
pub struct CommandResult {
    pub command: SlashCommand,
    pub message: Option<String>,
}

/// Parse a slash command from input text. Returns None if not a command.
pub fn parse_command(input: &str) -> Option<CommandResult> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = trimmed[1..].splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let arg = parts.get(1).map(|s| s.trim().to_string());

    let command = match cmd.as_str() {
        "help" | "h" => SlashCommand::Help,
        "clear" | "cls" => SlashCommand::Clear,
        "exit" | "quit" | "q" => SlashCommand::Exit,
        "status" => SlashCommand::Status,
        "model" | "m" => SlashCommand::Model(arg.clone()),
        "session" | "s" => SlashCommand::Session(arg.clone()),
        "think" | "thinking" => SlashCommand::Think(arg.clone()),
        "verbose" | "v" => SlashCommand::Verbose,
        "usage" | "u" => SlashCommand::Usage,
        "abort" => SlashCommand::Abort,
        _ => {
            return Some(CommandResult {
                command: SlashCommand::Help,
                message: Some(format!("Unknown command: /{}", cmd)),
            });
        }
    };

    Some(CommandResult { command, message: None })
}
