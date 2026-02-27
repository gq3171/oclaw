use crate::wizard::{error, get_config_dir, info};
use oclaw_config::ConfigManager;

pub struct ProviderSetup;

impl ProviderSetup {
    pub fn run() -> Result<(), String> {
        info("=== LLM Provider Status ===");
        println!();

        info("LLM providers can be configured via environment variables:");
        println!();
        println!("  OPENAI_API_KEY      - OpenAI API Key");
        println!("  ANTHROPIC_API_KEY  - Anthropic API Key");
        println!("  GOOGLE_AI_API_KEY  - Google AI API Key");
        println!("  OLLAMA_BASE_URL    - Ollama URL (default: http://localhost:11434)");
        println!("  COHERE_API_KEY    - Cohere API Key");
        println!();

        Self::show_current_status()?;

        Ok(())
    }

    fn show_current_status() -> Result<(), String> {
        let config_path = get_config_dir().join("config.json");

        if !config_path.exists() {
            info("No configuration file found.");
            return Ok(());
        }

        let mut manager = ConfigManager::new(config_path);
        if let Err(e) = manager.load() {
            error(&format!("Failed to load config: {}", e));
            return Ok(());
        }

        let config = manager.config();

        println!();
        info("Gateway Configuration:");
        println!("=====================");

        if let Some(gateway) = &config.gateway {
            println!("  Port: {}", gateway.port.unwrap_or(8080));
            println!("  Bind: {}", gateway.bind.as_deref().unwrap_or("0.0.0.0"));
            println!(
                "  Auth: {}",
                if gateway.auth.is_some() {
                    "Enabled"
                } else {
                    "Disabled"
                }
            );

            if let Some(auth) = &gateway.auth
                && auth.token.is_some()
            {
                println!("  Token: ***configured***");
            }
        } else {
            println!("  Not configured");
        }

        if let Some(channels) = &config.channels {
            println!();
            info("Channels Configuration:");
            println!("======================");

            if channels.webchat.is_some() {
                println!("  WebChat: enabled");
            }
            if channels.whatsapp.is_some() {
                println!("  WhatsApp: enabled");
            }
            if channels.telegram.is_some() {
                println!("  Telegram: enabled");
            }
            if channels.discord.is_some() {
                println!("  Discord: enabled");
            }
        }

        Ok(())
    }

    pub fn check_config() -> Result<(), String> {
        Self::show_current_status()
    }
}
