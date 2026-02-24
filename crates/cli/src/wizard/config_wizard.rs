use crate::wizard::{
    error, get_config_dir, info, prompt, prompt_optional, prompt_password, prompt_yes_no,
    select_option, success,
};
use oclaws_config::settings::*;
use oclaws_config::{Config, ConfigManager};
use std::collections::HashMap;

pub struct ConfigWizard;

impl ConfigWizard {
    pub fn run() -> Result<Config, String> {
        super::welcome();
        info("Full configuration wizard. Empty input keeps current value.");
        println!();

        let config_path = get_config_dir().join("config.json");
        let mut manager = ConfigManager::new(config_path);
        let mut config = if manager.load().is_ok() {
            info("Loaded existing configuration.");
            manager.config().clone()
        } else {
            info("No existing config found, starting fresh.");
            Config::default()
        };

        loop {
            let choice = select_option(
                "Configuration category:",
                &[
                    "Gateway",
                    "Models",
                    "Channels",
                    "Browser",
                    "Cron",
                    "Logging",
                    "Advanced",
                    "Save & Exit",
                ],
                7,
            );
            match choice {
                0 => Self::configure_gateway(&mut config),
                1 => Self::configure_models(&mut config),
                2 => Self::configure_channels(&mut config),
                3 => Self::configure_browser(&mut config),
                4 => Self::configure_cron(&mut config),
                5 => Self::configure_logging(&mut config),
                6 => Self::configure_advanced(&mut config),
                _ => {
                    let errors = config.validate();
                    if !errors.is_empty() {
                        for e in &errors {
                            error(e);
                        }
                        if !prompt_yes_no("Save anyway?", false) {
                            continue;
                        }
                    }
                    Self::save_config(&config)?;
                    break;
                }
            }
        }

        Ok(config)
    }

    fn configure_gateway(config: &mut Config) {
        info("=== Gateway ===");
        let gw = config.gateway.get_or_insert_with(Gateway::default);

        if let Some(v) = prompt_optional("Port", gw.port.map(|p| p.to_string()).as_deref())
            && let Ok(p) = v.parse::<i32>()
        {
            gw.port = Some(p);
        }
        gw.bind = prompt_optional("Bind address", gw.bind.as_deref());
        gw.mode = prompt_optional("Mode (server/client)", gw.mode.as_deref());

        if prompt_yes_no("Configure auth?", gw.auth.is_some()) {
            let auth = gw.auth.get_or_insert(GatewayAuth {
                mode: None, token: None, password: None,
                allow_tailscale: None, rate_limit: None, trusted_proxy: None,
            });
            auth.mode = prompt_optional("Auth mode (none/token/password)", auth.mode.as_deref());
            if auth.mode.as_deref() == Some("token") {
                let t = prompt_password("Token");
                if !t.is_empty() { auth.token = Some(t); }
            } else if auth.mode.as_deref() == Some("password") {
                let p = prompt_password("Password");
                if !p.is_empty() { auth.password = Some(p); }
            }
        }

        if prompt_yes_no("Configure TLS?", gw.tls.is_some()) {
            let tls = gw.tls.get_or_insert(GatewayTls {
                enabled: None, auto_generate: None, cert_path: None, key_path: None, ca_path: None,
            });
            tls.enabled = Some(prompt_yes_no("TLS enabled?", tls.enabled.unwrap_or(false)));
            tls.cert_path = prompt_optional("Cert path", tls.cert_path.as_deref());
            tls.key_path = prompt_optional("Key path", tls.key_path.as_deref());
        }

        if prompt_yes_no("Configure Tailscale?", gw.tailscale.is_some()) {
            let ts = gw.tailscale.get_or_insert(Tailscale { mode: None, reset_on_exit: None });
            ts.mode = prompt_optional("Tailscale mode", ts.mode.as_deref());
        }

        if prompt_yes_no("Configure reload?", gw.reload.is_some()) {
            let r = gw.reload.get_or_insert(GatewayReload { mode: None, debounce_ms: None });
            r.mode = prompt_optional("Reload mode", r.mode.as_deref());
        }

        success("Gateway configured");
    }

    fn configure_models(config: &mut Config) {
        info("=== Models ===");
        let models = config.models.get_or_insert_with(ModelsConfig::default);
        let providers = models.providers.get_or_insert_with(HashMap::new);

        loop {
            let mut names: Vec<String> = providers.keys().cloned().collect();
            names.sort();
            let mut opts: Vec<String> = names.iter().map(|n| format!("Edit: {}", n)).collect();
            opts.push("Add provider".into());
            opts.push("Remove provider".into());
            opts.push("Back".into());

            let choice = select_option("Model providers:", &opts, opts.len() - 1);
            if choice < names.len() {
                let name = &names[choice];
                let p = providers.get_mut(name).unwrap();
                Self::edit_provider(p);
            } else if choice == names.len() {
                let name = prompt("Provider name (e.g. openai)");
                if name.is_empty() { continue; }
                let mut p = ModelProvider {
                    provider: name.clone(), api_key: None, base_url: None,
                    model: None, max_tokens: None, temperature: None,
                    max_concurrency: None, headers: None, fallback: None,
                };
                Self::edit_provider(&mut p);
                providers.insert(name, p);
            } else if choice == names.len() + 1 {
                let name = prompt("Provider name to remove");
                if providers.remove(&name).is_some() {
                    success(&format!("Removed '{}'", name));
                }
            } else {
                break;
            }
        }
    }

    fn edit_provider(p: &mut ModelProvider) {
        p.provider = prompt_optional("Provider type", Some(&p.provider)).unwrap_or_default();
        let key = prompt_password("API key (enter to keep)");
        if !key.is_empty() { p.api_key = Some(key); }
        p.base_url = prompt_optional("Base URL", p.base_url.as_deref());
        p.model = prompt_optional("Model", p.model.as_deref());
        if let Some(v) = prompt_optional("Max tokens", p.max_tokens.map(|t| t.to_string()).as_deref()) {
            p.max_tokens = v.parse().ok();
        }
        if let Some(v) = prompt_optional("Temperature", p.temperature.map(|t| t.to_string()).as_deref()) {
            p.temperature = v.parse().ok();
        }
    }

    fn configure_channels(config: &mut Config) {
        info("=== Channels ===");
        let ch = config.channels.get_or_insert_with(Channels::default);

        let channel_names = [
            "Telegram", "Discord", "Slack", "Webchat", "Matrix",
            "Signal", "Line", "Mattermost", "Google Chat", "Back",
        ];
        loop {
            let choice = select_option("Channel:", &channel_names, channel_names.len() - 1);
            match choice {
                0 => {
                    let t = ch.telegram.get_or_insert(TelegramChannel { enabled: None, bot_token: None, api_url: None });
                    t.enabled = Some(prompt_yes_no("Enabled?", t.enabled.unwrap_or(false)));
                    let tok = prompt_password("Bot token (enter to keep)");
                    if !tok.is_empty() { t.bot_token = Some(tok); }
                    t.api_url = prompt_optional("API URL", t.api_url.as_deref());
                }
                1 => {
                    let d = ch.discord.get_or_insert(DiscordChannel { enabled: None, bot_token: None, guild_id: None, channel_ids: None });
                    d.enabled = Some(prompt_yes_no("Enabled?", d.enabled.unwrap_or(false)));
                    let tok = prompt_password("Bot token (enter to keep)");
                    if !tok.is_empty() { d.bot_token = Some(tok); }
                    d.guild_id = prompt_optional("Guild ID", d.guild_id.as_deref());
                }
                2 => {
                    let s = ch.slack.get_or_insert(SlackChannel { enabled: None, bot_token: None, signing_secret: None, channel_ids: None, webhook_url: None });
                    s.enabled = Some(prompt_yes_no("Enabled?", s.enabled.unwrap_or(false)));
                    let tok = prompt_password("Bot token (enter to keep)");
                    if !tok.is_empty() { s.bot_token = Some(tok); }
                    let sec = prompt_password("Signing secret (enter to keep)");
                    if !sec.is_empty() { s.signing_secret = Some(sec); }
                    s.webhook_url = prompt_optional("Webhook URL", s.webhook_url.as_deref());
                }
                3 => {
                    let w = ch.webchat.get_or_insert(WebchatChannel { enabled: None, auth: None });
                    w.enabled = Some(prompt_yes_no("Enabled?", w.enabled.unwrap_or(false)));
                }
                4 => {
                    let m = ch.matrix.get_or_insert(MatrixChannel { enabled: None, homeserver: None, user_id: None, access_token: None, device_id: None, room_id: None });
                    m.enabled = Some(prompt_yes_no("Enabled?", m.enabled.unwrap_or(false)));
                    m.homeserver = prompt_optional("Homeserver", m.homeserver.as_deref());
                    m.user_id = prompt_optional("User ID", m.user_id.as_deref());
                    let tok = prompt_password("Access token (enter to keep)");
                    if !tok.is_empty() { m.access_token = Some(tok); }
                    m.room_id = prompt_optional("Room ID", m.room_id.as_deref());
                }
                5 => {
                    let s = ch.signal.get_or_insert(SignalChannel { enabled: None, phone_number: None, api_url: None, signal_cli_path: None });
                    s.enabled = Some(prompt_yes_no("Enabled?", s.enabled.unwrap_or(false)));
                    s.phone_number = prompt_optional("Phone number", s.phone_number.as_deref());
                    s.api_url = prompt_optional("API URL", s.api_url.as_deref());
                }
                6 => {
                    let l = ch.line.get_or_insert(LineChannel { enabled: None, channel_access_token: None, channel_secret: None, user_id: None });
                    l.enabled = Some(prompt_yes_no("Enabled?", l.enabled.unwrap_or(false)));
                    let tok = prompt_password("Channel access token (enter to keep)");
                    if !tok.is_empty() { l.channel_access_token = Some(tok); }
                    let sec = prompt_password("Channel secret (enter to keep)");
                    if !sec.is_empty() { l.channel_secret = Some(sec); }
                }
                7 => {
                    let m = ch.mattermost.get_or_insert(MattermostChannel { enabled: None, server_url: None, access_token: None, team_id: None, channel_id: None });
                    m.enabled = Some(prompt_yes_no("Enabled?", m.enabled.unwrap_or(false)));
                    m.server_url = prompt_optional("Server URL", m.server_url.as_deref());
                    let tok = prompt_password("Access token (enter to keep)");
                    if !tok.is_empty() { m.access_token = Some(tok); }
                    m.team_id = prompt_optional("Team ID", m.team_id.as_deref());
                    m.channel_id = prompt_optional("Channel ID", m.channel_id.as_deref());
                }
                8 => {
                    let g = ch.google_chat.get_or_insert(GoogleChatChannel { enabled: None, space_name: None, service_account_json: None });
                    g.enabled = Some(prompt_yes_no("Enabled?", g.enabled.unwrap_or(false)));
                    g.space_name = prompt_optional("Space name", g.space_name.as_deref());
                }
                _ => break,
            }
            success("Channel updated");
        }
    }

    fn configure_browser(config: &mut Config) {
        info("=== Browser ===");
        let b = config.browser.get_or_insert_with(Browser::default);
        b.enabled = Some(prompt_yes_no("Enabled?", b.enabled.unwrap_or(false)));
        b.headless = Some(prompt_yes_no("Headless?", b.headless.unwrap_or(true)));
        b.cdp_url = prompt_optional("CDP URL", b.cdp_url.as_deref());
        b.no_sandbox = Some(prompt_yes_no("No sandbox?", b.no_sandbox.unwrap_or(false)));
        b.evaluate_enabled = Some(prompt_yes_no("Evaluate enabled?", b.evaluate_enabled.unwrap_or(false)));
        success("Browser configured");
    }

    fn configure_cron(config: &mut Config) {
        info("=== Cron ===");
        let c = config.cron.get_or_insert(Cron {
            enabled: None, store: None, max_concurrent_runs: None,
            webhook: None, webhook_token: None, session_retention: None,
        });
        c.enabled = Some(prompt_yes_no("Enabled?", c.enabled.unwrap_or(false)));
        if let Some(v) = prompt_optional("Max concurrent runs", c.max_concurrent_runs.map(|n| n.to_string()).as_deref()) {
            c.max_concurrent_runs = v.parse().ok();
        }
        c.store = prompt_optional("Store path", c.store.as_deref());
        c.webhook = prompt_optional("Webhook URL", c.webhook.as_deref());
        let tok = prompt_password("Webhook token (enter to keep)");
        if !tok.is_empty() { c.webhook_token = Some(tok); }
        success("Cron configured");
    }

    fn configure_logging(config: &mut Config) {
        info("=== Logging ===");
        let l = config.logging.get_or_insert(Logging {
            level: None, file: None, console_level: None,
            console_style: None, redact_sensitive: None, redact_patterns: None,
        });
        l.level = prompt_optional("Level (trace/debug/info/warn/error)", l.level.as_deref());
        l.file = prompt_optional("Log file path", l.file.as_deref());
        l.console_level = prompt_optional("Console level", l.console_level.as_deref());
        l.redact_sensitive = prompt_optional("Redact sensitive (true/false)", l.redact_sensitive.as_deref());
        success("Logging configured");
    }

    fn configure_advanced(config: &mut Config) {
        let opts = [
            "Diagnostics/OTel", "Talk", "Web/Reconnect", "UI",
            "Update", "Env", "Media", "Canvas Host", "Discovery", "Back",
        ];
        loop {
            let choice = select_option("Advanced:", &opts, opts.len() - 1);
            match choice {
                0 => {
                    let d = config.diagnostics.get_or_insert(Diagnostics { enabled: None, flags: None, otel: None, cache_trace: None });
                    d.enabled = Some(prompt_yes_no("Diagnostics enabled?", d.enabled.unwrap_or(false)));
                    if prompt_yes_no("Configure OTel?", d.otel.is_some()) {
                        let o = d.otel.get_or_insert(Otel {
                            enabled: None, endpoint: None, protocol: None, headers: None,
                            service_name: None, traces: None, metrics: None, logs: None,
                            sample_rate: None, flush_interval_ms: None,
                        });
                        o.enabled = Some(prompt_yes_no("OTel enabled?", o.enabled.unwrap_or(false)));
                        o.endpoint = prompt_optional("Endpoint", o.endpoint.as_deref());
                        o.service_name = prompt_optional("Service name", o.service_name.as_deref());
                    }
                }
                1 => {
                    let t = config.talk.get_or_insert(Talk {
                        voice_id: None, voice_aliases: None, model_id: None,
                        output_format: None, api_key: None, interrupt_on_speech: None,
                    });
                    t.voice_id = prompt_optional("Voice ID", t.voice_id.as_deref());
                    t.model_id = prompt_optional("Model ID", t.model_id.as_deref());
                    let key = prompt_password("API key (enter to keep)");
                    if !key.is_empty() { t.api_key = Some(key); }
                }
                2 => {
                    let w = config.web.get_or_insert(Web { enabled: None, heartbeat_seconds: None, reconnect: None });
                    w.enabled = Some(prompt_yes_no("Web enabled?", w.enabled.unwrap_or(false)));
                    if prompt_yes_no("Configure reconnect?", w.reconnect.is_some()) {
                        let r = w.reconnect.get_or_insert(Reconnect {
                            initial_ms: None, max_ms: None, factor: None, jitter: None, max_attempts: None,
                        });
                        if let Some(v) = prompt_optional("Initial ms", r.initial_ms.map(|n| n.to_string()).as_deref()) {
                            r.initial_ms = v.parse().ok();
                        }
                        if let Some(v) = prompt_optional("Max ms", r.max_ms.map(|n| n.to_string()).as_deref()) {
                            r.max_ms = v.parse().ok();
                        }
                    }
                }
                3 => {
                    let u = config.ui.get_or_insert(Ui { seam_color: None, assistant: None });
                    u.seam_color = prompt_optional("Seam color", u.seam_color.as_deref());
                    if prompt_yes_no("Configure assistant?", u.assistant.is_some()) {
                        let a = u.assistant.get_or_insert(Assistant { name: None, avatar: None });
                        a.name = prompt_optional("Name", a.name.as_deref());
                        a.avatar = prompt_optional("Avatar URL", a.avatar.as_deref());
                    }
                }
                4 => {
                    let u = config.update.get_or_insert(Update { channel: None, check_on_start: None });
                    u.channel = prompt_optional("Update channel", u.channel.as_deref());
                    u.check_on_start = Some(prompt_yes_no("Check on start?", u.check_on_start.unwrap_or(true)));
                }
                5 => {
                    let e = config.env.get_or_insert(Env { shell_env: None, vars: None });
                    if prompt_yes_no("Configure shell env?", e.shell_env.is_some()) {
                        let s = e.shell_env.get_or_insert(ShellEnv { enabled: None, timeout_ms: None });
                        s.enabled = Some(prompt_yes_no("Shell env enabled?", s.enabled.unwrap_or(false)));
                    }
                }
                6 => {
                    let m = config.media.get_or_insert(Media { preserve_filenames: None });
                    m.preserve_filenames = Some(prompt_yes_no("Preserve filenames?", m.preserve_filenames.unwrap_or(false)));
                }
                7 => {
                    let c = config.canvas_host.get_or_insert(CanvasHost { enabled: None, root: None, port: None, live_reload: None });
                    c.enabled = Some(prompt_yes_no("Canvas host enabled?", c.enabled.unwrap_or(false)));
                    c.root = prompt_optional("Root path", c.root.as_deref());
                    if let Some(v) = prompt_optional("Port", c.port.map(|p| p.to_string()).as_deref()) {
                        c.port = v.parse().ok();
                    }
                }
                8 => {
                    let d = config.discovery.get_or_insert(Discovery { wide_area: None, mdns: None });
                    if prompt_yes_no("Configure mDNS?", d.mdns.is_some()) {
                        let m = d.mdns.get_or_insert(MdnsDiscovery { mode: None });
                        m.mode = prompt_optional("mDNS mode", m.mode.as_deref());
                    }
                }
                _ => break,
            }
            success("Updated");
        }
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
        println!("\nCurrent Configuration:\n=====================\n{}", content);
        Ok(())
    }
}
