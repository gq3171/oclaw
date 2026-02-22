use crate::wizard::{get_config_dir, info, prompt, prompt_yes_no, success};
use oclaws_config::Config;

pub struct ChannelWizard;

impl ChannelWizard {
    pub fn run() -> Result<Config, String> {
        info("=== Channel Setup Wizard ===");
        println!();

        let mut config = Config::default();

        let enable_webchat = prompt_yes_no("Enable WebChat?", true);
        if enable_webchat {
            info("WebChat will be enabled.");
        }

        info("Channel configuration complete.");
        Ok(config)
    }
}
