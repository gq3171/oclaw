/// Parsed command from user input.
#[derive(Debug, Clone)]
pub enum Command {
    Set { key: String, value: String },
    Compact,
    Spawn { name: String, prompt: String },
    Model { model: String },
    Reset,
    Verbose { enabled: bool },
    Think { level: String },
    Help,
    Status,
    Custom { name: String, args: Vec<String> },
}

/// Result of executing a command.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub response: String,
    /// If `false`, the command was fully handled — don't forward to LLM.
    pub should_continue: bool,
}

/// Parses `/command arg1 arg2` style input.
pub struct CommandParser {
    pub prefix: char,
}

impl Default for CommandParser {
    fn default() -> Self {
        Self { prefix: '/' }
    }
}

impl CommandParser {
    pub fn parse(&self, input: &str) -> Option<Command> {
        let trimmed = input.trim();
        if !trimmed.starts_with(self.prefix) {
            return None;
        }
        let without_prefix = &trimmed[self.prefix.len_utf8()..];
        let mut parts = without_prefix.splitn(3, ' ');
        let name = parts.next()?.to_lowercase();
        let arg1 = parts.next().unwrap_or("").to_string();
        let arg2 = parts.next().unwrap_or("").to_string();

        match name.as_str() {
            "set" => {
                if arg1.is_empty() {
                    return None;
                }
                Some(Command::Set { key: arg1, value: arg2 })
            }
            "compact" => Some(Command::Compact),
            "spawn" => {
                if arg1.is_empty() {
                    return None;
                }
                Some(Command::Spawn { name: arg1, prompt: arg2 })
            }
            "model" => {
                if arg1.is_empty() {
                    return None;
                }
                Some(Command::Model { model: arg1 })
            }
            "reset" => Some(Command::Reset),
            "verbose" => {
                let enabled = matches!(arg1.as_str(), "on" | "true" | "1" | "yes");
                Some(Command::Verbose { enabled })
            }
            "think" => Some(Command::Think {
                level: if arg1.is_empty() { "high".into() } else { arg1 },
            }),
            "help" => Some(Command::Help),
            "status" => Some(Command::Status),
            other => {
                let mut args = Vec::new();
                if !arg1.is_empty() { args.push(arg1); }
                if !arg2.is_empty() { args.push(arg2); }
                Some(Command::Custom { name: other.to_string(), args })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set() {
        let p = CommandParser::default();
        let cmd = p.parse("/set temperature 0.7").unwrap();
        assert!(matches!(cmd, Command::Set { key, value } if key == "temperature" && value == "0.7"));
    }

    #[test]
    fn parse_compact() {
        let p = CommandParser::default();
        assert!(matches!(p.parse("/compact"), Some(Command::Compact)));
    }

    #[test]
    fn parse_model() {
        let p = CommandParser::default();
        let cmd = p.parse("/model gpt-4").unwrap();
        assert!(matches!(cmd, Command::Model { model } if model == "gpt-4"));
    }

    #[test]
    fn non_command_returns_none() {
        let p = CommandParser::default();
        assert!(p.parse("hello world").is_none());
    }

    #[test]
    fn custom_command() {
        let p = CommandParser::default();
        let cmd = p.parse("/foo bar baz").unwrap();
        assert!(matches!(cmd, Command::Custom { name, args } if name == "foo" && args.len() == 2));
    }
}
