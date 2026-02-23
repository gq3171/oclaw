use crate::wizard::{error, get_config_dir, info, prompt, prompt_yes_no, success};
use oclaws_config::settings::Gateway;
use oclaws_config::Config;

pub struct ConfigWizard;

impl ConfigWizard {
    pub fn run() -> Result<Config, String> {
        super::welcome();

        info("This wizard will help you configure OCLAWS for first-time setup.");
        println!();

        let mut config = Config::default();

        config = Self::configure_gateway(config)?;

        if prompt_yes_no("Save configuration now?", true) {
            Self::save_config(&config)?;
        }

        Ok(config)
    }

    fn configure_gateway(mut config: Config) -> Result<Config, String> {
        info("=== Gateway Configuration ===");
        println!();

        let port: u16 = loop {
            let input = prompt("WebSocket port (default: 8080)");
            if input.is_empty() {
                break 8080;
            }
            match input.parse() {
                Ok(p) if p > 1024 => break p,
                _ => error("Please enter a valid port number (> 1024)"),
            }
        };

        let bind = prompt("Bind address (default: 0.0.0.0)");
        let bind = if bind.is_empty() {
            "0.0.0.0".to_string()
        } else {
            bind
        };

        let enable_auth = prompt_yes_no("Enable authentication?", true);

        let token = if enable_auth {
            let input = prompt("Access token (leave empty to generate)");
            if input.is_empty() {
                let token = uuid::Uuid::new_v4().to_string();
                info(&format!("Generated token: {}", token));
                Some(token)
            } else {
                Some(input)
            }
        } else {
            None
        };

        config.gateway = Some(Gateway {
            port: Some(port as i32),
            mode: Some("server".to_string()),
            bind: Some(bind),
            custom_bind_host: None,
            control_ui: None,
            auth: token.map(|t| oclaws_config::settings::GatewayAuth {
                mode: Some("token".to_string()),
                token: Some(t),
                password: None,
                allow_tailscale: None,
                rate_limit: None,
                trusted_proxy: None,
            }),
            trusted_proxies: None,
            allow_real_ip_fallback: None,
            tools: None,
            channel_health_check_minutes: None,
            tailscale: None,
            remote: None,
            reload: None,
            tls: None,
            http: None,
        });

        success("Gateway configuration saved");
        Ok(config)
    }

    fn save_config(config: &Config) -> Result<(), String> {
        let config_dir = get_config_dir();
        let config_path = config_dir.join("config.json");

        let content = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        std::fs::write(&config_path, content)
            .map_err(|e| format!("Failed to write config: {}", e))?;

        success(&format!("Configuration saved to {:?}", config_path));

        info("Run 'oclaws start' to start the server.");

        Ok(())
    }

    #[allow(dead_code)]
    pub fn show_current_config() -> Result<(), String> {
        let config_path = get_config_dir().join("config.json");

        if !config_path.exists() {
            error("No configuration found. Run 'oclaws wizard' to set up OCLAWS.");
            return Ok(());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config: {}", e))?;

        println!();
        println!("Current Configuration:");
        println!("=====================");
        println!("{}", content);

        Ok(())
    }
}
