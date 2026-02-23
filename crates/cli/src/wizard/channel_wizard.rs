use crate::wizard::{get_config_dir, info, prompt, prompt_yes_no, success};
use oclaws_config::Config;
use oclaws_config::settings::*;

pub struct ChannelWizard;

impl ChannelWizard {
    pub fn run() -> Result<Config, String> {
        info("=== Channel Setup Wizard ===");
        println!();

        let mut channels = Channels::default();

        if prompt_yes_no("Enable WebChat?", true) {
            channels.webchat = Some(WebchatChannel { enabled: Some(true), auth: None });
        }

        if prompt_yes_no("Enable Discord?", false) {
            let token = prompt("Discord bot token");
            let guild = prompt("Guild ID (optional, press Enter to skip)");
            channels.discord = Some(DiscordChannel {
                enabled: Some(true),
                bot_token: Some(token),
                guild_id: if guild.is_empty() { None } else { Some(guild) },
                channel_ids: None,
            });
        }

        if prompt_yes_no("Enable Slack?", false) {
            let token = prompt("Slack bot token");
            let secret = prompt("Signing secret");
            channels.slack = Some(SlackChannel {
                enabled: Some(true),
                bot_token: Some(token),
                signing_secret: Some(secret),
                channel_ids: None,
                webhook_url: None,
            });
        }

        if prompt_yes_no("Enable Telegram?", false) {
            let token = prompt("Telegram bot token");
            channels.telegram = Some(TelegramChannel {
                enabled: Some(true),
                bot_token: Some(token),
                api_url: None,
            });
        }

        let mut config = Self::load_or_default();
        config.channels = Some(channels);

        if prompt_yes_no("Save channel configuration?", true) {
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
        let dir = get_config_dir();
        let path = dir.join("config.json");
        let content = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize: {}", e))?;
        std::fs::write(&path, content)
            .map_err(|e| format!("Failed to write: {}", e))?;
        success(&format!("Channel config saved to {:?}", path));
        Ok(())
    }
}
