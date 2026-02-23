use crate::wizard::{get_config_dir, info, prompt, prompt_yes_no, success};
use oclaws_config::Config;

pub struct SkillWizard;

impl SkillWizard {
    pub fn run() -> Result<Config, String> {
        info("=== Skill Setup Wizard ===");
        println!();

        let skills = [
            ("calculator", "Math expressions"),
            ("json_formatter", "Format & validate JSON"),
            ("datetime", "Date/time operations"),
            ("hash", "SHA256/SHA512 hashing"),
            ("base64", "Encode/decode Base64"),
            ("regex", "Pattern matching"),
            ("url_parser", "Parse URL components"),
            ("text_transform", "Case/reverse/trim"),
        ];

        println!("Available skills:");
        for (i, (name, desc)) in skills.iter().enumerate() {
            println!("  {}. {:16} - {}", i + 1, name, desc);
        }
        println!();

        let selection = prompt("Select skills (comma-separated numbers, or 'all')");

        let enabled: Vec<String> = if selection == "all" || selection.is_empty() {
            skills.iter().map(|(n, _)| n.to_string()).collect()
        } else {
            selection.split(',')
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .filter(|&i| i >= 1 && i <= skills.len())
                .map(|i| skills[i - 1].0.to_string())
                .collect()
        };

        info(&format!("Enabled: {}", enabled.join(", ")));

        let mut config = Self::load_or_default();
        config.tools = Some(serde_json::json!({ "enabled": enabled }));

        if prompt_yes_no("Save skill configuration?", true) {
            Self::save(&config)?;
        }

        Ok(config)
    }

    fn load_or_default() -> Config {
        let path = get_config_dir().join("config.json");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(config: &Config) -> Result<(), String> {
        let path = get_config_dir().join("config.json");
        let content = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize: {}", e))?;
        std::fs::write(&path, content)
            .map_err(|e| format!("Failed to write: {}", e))?;
        success(&format!("Skill config saved to {:?}", path));
        Ok(())
    }
}
