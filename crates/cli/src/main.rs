use clap::{Parser, Subcommand};
use oclaws_gateway_core::GatewayServer;
use oclaws_config::{Config, ConfigManager};
use oclaws_config::settings::Gateway;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug, Level};
use tracing_subscriber::{FmtSubscriber, fmt};

mod wizard;

/// Feishu WebSocket protobuf frame (pbbp2.proto)
#[derive(Clone, PartialEq, prost::Message)]
struct FsFrame {
    #[prost(uint64, tag = "1")]
    seq_id: u64,
    #[prost(uint64, tag = "2")]
    log_id: u64,
    #[prost(int32, tag = "3")]
    service: i32,
    #[prost(int32, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<FsHeader>,
    #[prost(string, tag = "6")]
    payload_encoding: String,
    #[prost(string, tag = "7")]
    payload_type: String,
    #[prost(bytes = "vec", tag = "8")]
    payload: Vec<u8>,
    #[prost(string, tag = "9")]
    log_id_new: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct FsHeader {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Parser)]
#[command(name = "oclaws")]
#[command(version = "0.1.0")]
#[command(about = "OCLAWS - Open CLAW System", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    
    #[arg(short, long, default_value = "info", env = "OCLAWS_LOG_LEVEL")]
    log_level: String,

    #[arg(long, default_value = "text")]
    log_format: String,

    #[arg(short, long)]
    config: Option<String>,

    #[arg(long, default_value = "http://127.0.0.1:8081", env = "OCLAWS_GATEWAY_URL")]
    gateway_url: String,
}

#[derive(Subcommand)]
enum Commands {
    Start {
        #[arg(short, long)]
        port: Option<u16>,

        #[arg(short, long)]
        host: Option<String>,

        #[arg(long, default_value = "false")]
        http_only: bool,

        #[arg(long, default_value = "false")]
        ws_only: bool,
    },
    Config {
        #[arg(short, long)]
        path: Option<String>,
        
        #[command(subcommand)]
        action: ConfigAction,
    },
    Wizard {
        #[arg(short, long)]
        path: Option<String>,
        
        #[arg(long)]
        skip_existing: bool,
    },
    Channel {
        #[command(subcommand)]
        action: ChannelAction,
    },
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    Doctor {
        #[arg(long)]
        category: Option<String>,
    },
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },
    Agent {
        #[arg(short, long)]
        message: Option<String>,
        #[arg(short, long, default_value = "default")]
        model: String,
    },
    Sessions {
        #[command(subcommand)]
        action: SessionAction,
    },
    Models {
        #[command(subcommand)]
        action: ModelAction,
    },
    Message {
        #[arg(short, long)]
        session: Option<String>,
        text: String,
    },
    Status,
    Version,
    Tui,
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    Start {
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
    Stop,
    Status,
}

#[derive(Subcommand)]
enum ConfigAction {
    Init,
    Show,
    Validate,
}

#[derive(Subcommand)]
enum ChannelAction {
    Setup,
    List,
}

#[derive(Subcommand)]
enum PluginAction {
    List,
    Info { id: String },
    Enable { id: String },
    Disable { id: String },
}

#[derive(Subcommand)]
enum SkillAction {
    Setup,
    List {
        #[arg(long, default_value = "false")]
        eligible: bool,
    },
    Info {
        name: String,
    },
    Check {
        name: Option<String>,
    },
    Install {
        name: String,
        #[arg(long, default_value = "300")]
        timeout: u64,
    },
}

#[derive(Subcommand)]
enum ProviderAction {
    Setup,
    Status,
}

#[derive(Subcommand)]
enum SessionAction {
    List,
    Show { key: String },
    Delete { key: String },
}

#[derive(Subcommand)]
enum ModelAction {
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    
    let log_level = match cli.log_level.as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };
    
    if cli.log_format == "json" {
        fmt::Subscriber::builder()
            .json()
            .with_max_level(log_level)
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .init();
    } else {
        FmtSubscriber::builder()
            .with_max_level(log_level)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(true)
            .with_line_number(true)
            .init();
    }
    
    let Some(command) = cli.command else {
        // No subcommand — print help and exit
        use clap::CommandFactory;
        Cli::command().print_help().ok();
        println!();
        return Ok(());
    };

    match command {
        Commands::Start { port, host, http_only, ws_only } => {
            // 1. Load config
            let config_path = cli.config
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
            let mut manager = ConfigManager::new(config_path.clone());
            let config = if manager.load().is_ok() {
                manager.config_mut().apply_env_overrides();
                info!("Loaded config from {:?}", config_path);
                manager.config().clone()
            } else {
                warn!("No config found, using defaults");
                Config::default()
            };

            // 2. Build gateway config with CLI overrides
            let mut gateway_config = config.gateway.clone().unwrap_or_default();
            if let Some(p) = port {
                gateway_config.port = Some(p as i32);
            }
            if let Some(ref h) = host {
                gateway_config.bind = Some(h.clone());
            }
            let port = gateway_config.port.unwrap_or(8080) as u16;

            // 3. Create LLM provider from config
            let llm_provider = create_llm_provider(&config);

            // 3b. Create tool registry with browser config
            let mut tool_registry = oclaws_tools_core::tool::ToolRegistry::new();
            if let Some(ref browser) = config.browser {
                tool_registry.configure_browser(
                    browser.cdp_url.as_deref(),
                    browser.executable_path.as_deref(),
                    browser.headless,
                );
            }
            let tool_registry = Arc::new(tool_registry);
            info!("Tool registry initialized with {} tools", tool_registry.list().len());

            // 3c. Load plugins and create registrations
            let plugin_registrations = {
                let loader = oclaws_plugin_core::PluginLoader::new();
                let plugins = loader.discover_plugins();
                let regs = Arc::new(oclaws_plugin_core::PluginRegistrations::new());
                if !plugins.is_empty() {
                    info!("Discovered {} plugin(s)", plugins.len());
                    for p in &plugins {
                        info!("  Plugin: {} v{}", p.manifest.id, p.manifest.version);
                    }
                }
                Some(regs)
            };

            // 3d. Start cron service if enabled
            let cron_service = if config.cron.as_ref().and_then(|c| c.enabled).unwrap_or(false) {
                let store_path = config.cron.as_ref()
                    .and_then(|c| c.store.as_ref())
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(oclaws_cron_core::CronStore::default_path);
                let store = oclaws_cron_core::CronStore::new(store_path);
                let svc = Arc::new(oclaws_cron_core::CronService::new(store));
                info!("Cron service initialized");
                Some(svc)
            } else {
                None
            };

            // 4. Create channel manager from config
            let channel_manager = if let Some(ref channels) = config.channels {
                let cm = oclaws_channel_core::ChannelManager::from_config(channels).await;
                cm.connect_all().await.ok();
                info!("Channel manager initialized");
                Some(Arc::new(RwLock::new(cm)))
            } else {
                None
            };

            // 4b. Start Telegram long-polling if configured
            if let Some(ref channels) = config.channels
                && let Some(ref tg) = channels.telegram
                && tg.enabled.unwrap_or(false)
                && let Some(ref bot_token) = tg.bot_token
            {
                let token = bot_token.clone();
                let provider = llm_provider.clone();
                let cm = channel_manager.clone();
                let tr = tool_registry.clone();
                tokio::spawn(async move {
                    telegram_poll_loop(token, provider, cm, tr).await;
                });
            }

            // 4c. Start Feishu WebSocket long connection if configured
            if let Some(ref channels) = config.channels
                && let Some(ref fs) = channels.feishu
                && fs.enabled.unwrap_or(false)
                && let (Some(app_id), Some(app_secret)) = (&fs.app_id, &fs.app_secret)
            {
                let aid = app_id.clone();
                let asec = app_secret.clone();
                let provider = llm_provider.clone();
                let cm = channel_manager.clone();
                let tr = tool_registry.clone();
                tokio::spawn(async move {
                    feishu_ws_loop(aid, asec, provider, cm, tr).await;
                });
            }

            // 5. Start config watcher
            let watcher = oclaws_config::ConfigWatcher::new(config_path.clone());
            let mut watch_rx = watcher.watch();
            tokio::spawn(async move {
                while watch_rx.changed().await.is_ok() {
                    info!("Config file changed, reload may be needed");
                }
            });

            // 6. Start servers
            let gateway_server = Arc::new(GatewayServer::new(port));

            if http_only {
                info!("Starting OCLAWS HTTP server on port {}", port);
                let server = build_http_server(HttpServerParams { port, gateway: gateway_config, gateway_server, llm_provider, channel_manager, tool_registry: tool_registry.clone(), plugin_registrations: plugin_registrations.clone(), cron_service: cron_service.clone(), full_config: config.clone(), config_path: config_path.clone() }).await?;                if let Err(e) = server.start().await {
                    error!("HTTP server error: {}", e);
                    return Err(anyhow::anyhow!(e));
                }
            } else if ws_only {
                info!("Starting OCLAWS WebSocket server on port {}", port);
                if let Err(e) = gateway_server.start().await {
                    error!("WebSocket server error: {}", e);
                    return Err(anyhow::anyhow!(e));
                }
            } else {
                info!("Starting OCLAWS gateway on port {} (WS) + {} (HTTP)", port, port + 1);
                let ws_server = gateway_server.clone();
                tokio::spawn(async move {
                    if let Err(e) = ws_server.start().await {
                        error!("WebSocket server error: {}", e);
                    }
                });

                let server = build_http_server(HttpServerParams { port: port + 1, gateway: gateway_config, gateway_server, llm_provider, channel_manager, tool_registry: tool_registry.clone(), plugin_registrations: plugin_registrations.clone(), cron_service: cron_service.clone(), full_config: config.clone(), config_path: config_path.clone() }).await?;
                tokio::spawn(async move {
                    if let Err(e) = server.start().await {
                        error!("HTTP server error: {}", e);
                    }
                });

                tokio::signal::ctrl_c().await?;
                info!("Shutting down...");
            }
        }
        
        Commands::Config { path, action } => {
            let config_path = path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
            
            match action {
                ConfigAction::Init => {
                    info!("Initializing config at {:?}", config_path);
                    
                    let config = Config::default();
                    let content = serde_json::to_string_pretty(&config)?;
                    std::fs::write(&config_path, content)?;
                    
                    info!("Config initialized at {:?}", config_path);
                }
                ConfigAction::Show => {
                    let mut manager = ConfigManager::new(config_path);
                    if let Err(e) = manager.load() {
                        error!("Failed to load config: {}", e);
                        return Err(anyhow::anyhow!(e));
                    }
                    
                    let content = serde_json::to_string_pretty(manager.config())?;
                    println!("{}", content);
                }
                ConfigAction::Validate => {
                    let mut manager = ConfigManager::new(config_path);
                    if let Err(e) = manager.load() {
                        error!("Config validation failed: {}", e);
                        return Err(anyhow::anyhow!(e));
                    }
                    
                    info!("Config is valid");
                }
            }
        }
        
        Commands::Wizard { path, skip_existing } => {
            use wizard::ConfigWizard;
            use wizard::{success, error};
            
            let config_path = path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
            
            if !skip_existing {
                let mut manager = ConfigManager::new(config_path);
                if manager.load().is_ok()
                    && !wizard::prompt_yes_no("Configuration already exists. Overwrite?", false) {
                        info!("Aborted.");
                        return Ok(());
                    }
            }
            
            match ConfigWizard::run() {
                Ok(_) => {
                    success("Setup complete! Run 'oclaws start' to start the server.");
                }
                Err(e) => {
                    error(&format!("Setup failed: {}", e));
                    return Err(anyhow::anyhow!(e));
                }
            }
        }
        
        Commands::Channel { action } => {
            use wizard::ChannelWizard;
            
            match action {
                ChannelAction::Setup => {
                    if let Err(e) = ChannelWizard::run() {
                        tracing::error!("Channel setup failed: {}", e);
                        return Err(anyhow::anyhow!(e));
                    }
                }
                ChannelAction::List => {
                    tracing::info!("Channel configuration:");
                    tracing::info!("Use 'oclaws channel setup' to configure channels.");
                }
            }
        }
        
        Commands::Skill { action } => {
            use oclaws_skills_core::{discovery, gates, installer};

            let workspace = std::env::current_dir().ok();
            match action {
                SkillAction::Setup => {
                    use wizard::SkillWizard;
                    if let Err(e) = SkillWizard::run() {
                        tracing::error!("Skill setup failed: {}", e);
                        return Err(anyhow::anyhow!(e));
                    }
                }
                SkillAction::List { eligible } => {
                    let skills = discovery::discover_skills(workspace.as_deref()).await;
                    if skills.is_empty() {
                        println!("No skills found.");
                        return Ok(());
                    }
                    for s in &skills {
                        let gate = if eligible {
                            let r = gates::check_gates(&s.manifest, &|_| false);
                            if !r.passed { continue; }
                            " [eligible]"
                        } else { "" };
                        println!("  {:?}  {}  {}{}", s.tier, s.manifest.name, s.manifest.description, gate);
                    }
                }
                SkillAction::Info { name } => {
                    let skills = discovery::discover_skills(workspace.as_deref()).await;
                    match skills.iter().find(|s| s.manifest.name == name) {
                        Some(s) => {
                            println!("Name: {}", s.manifest.name);
                            println!("Tier: {:?}", s.tier);
                            println!("Description: {}", s.manifest.description);
                            println!("Source: {}", s.manifest.source_dir);
                            if let Some(meta) = &s.manifest.metadata {
                                if let Some(oc) = &meta.openclaw {
                                    if let Some(req) = &oc.requires {
                                        if !req.bins.is_empty() { println!("Requires bins: {}", req.bins.join(", ")); }
                                        if !req.env.is_empty() { println!("Requires env: {}", req.env.join(", ")); }
                                    }
                                    if !oc.install.is_empty() {
                                        println!("Install specs: {}", oc.install.len());
                                    }
                                }
                            }
                        }
                        None => println!("Skill '{}' not found", name),
                    }
                }
                SkillAction::Check { name } => {
                    let skills = discovery::discover_skills(workspace.as_deref()).await;
                    let to_check: Vec<_> = if let Some(ref n) = name {
                        skills.iter().filter(|s| s.manifest.name == *n).collect()
                    } else {
                        skills.iter().collect()
                    };
                    for s in to_check {
                        let r = gates::check_gates(&s.manifest, &|_| false);
                        let status = if r.passed { "PASS" } else { "FAIL" };
                        print!("  [{}] {}", status, s.manifest.name);
                        if !r.missing_bins.is_empty() { print!("  missing bins: {}", r.missing_bins.join(", ")); }
                        if !r.missing_env.is_empty() { print!("  missing env: {}", r.missing_env.join(", ")); }
                        if r.os_mismatch { print!("  OS mismatch"); }
                        println!();
                    }
                }
                SkillAction::Install { name, timeout } => {
                    let skills = discovery::discover_skills(workspace.as_deref()).await;
                    let Some(skill) = skills.iter().find(|s| s.manifest.name == name) else {
                        println!("Skill '{}' not found", name);
                        return Ok(());
                    };
                    let specs = skill.manifest.metadata.as_ref()
                        .and_then(|m| m.openclaw.as_ref())
                        .map(|oc| &oc.install[..])
                        .unwrap_or(&[]);
                    if specs.is_empty() {
                        println!("Skill '{}' has no install specs", name);
                        return Ok(());
                    }
                    for spec in specs {
                        println!("Installing {} ({})...", spec.kind, spec.formula.as_deref()
                            .or(spec.package.as_deref())
                            .or(spec.module.as_deref())
                            .or(spec.url.as_deref())
                            .unwrap_or("?"));
                        let r = installer::run_install(spec, timeout).await;
                        if r.ok {
                            println!("  OK");
                        } else {
                            println!("  FAILED: {}", r.message);
                            if !r.stderr.is_empty() { println!("  stderr: {}", r.stderr.lines().take(5).collect::<Vec<_>>().join("\n  ")); }
                        }
                    }
                }
            }
        }
        
        Commands::Doctor { category } => {
            use wizard::DoctorWizard;
            use oclaws_doctor_core::CheckCategory;
            
            if let Some(cat) = category {
                let cat = match cat.as_str() {
                    "system" => CheckCategory::System,
                    "network" => CheckCategory::Network,
                    "config" | "configuration" => CheckCategory::Configuration,
                    "dependencies" | "deps" => CheckCategory::Dependencies,
                    "storage" => CheckCategory::Storage,
                    "security" => CheckCategory::Security,
                    "performance" => CheckCategory::Performance,
                    _ => {
                        tracing::error!("Invalid category. Use: system, network, config, dependencies, storage, security, or performance");
                        return Ok(());
                    }
                };
                
                DoctorWizard::check_category(cat);
            } else {
                DoctorWizard::run();
            }
        }
        
        Commands::Provider { action } => {
            use wizard::ProviderSetup;
            
            match action {
                ProviderAction::Setup => {
                    if let Err(e) = ProviderSetup::run() {
                        error!("Provider setup failed: {}", e);
                        return Err(anyhow::anyhow!(e));
                    }
                }
                ProviderAction::Status => {
                    if let Err(e) = ProviderSetup::check_config() {
                        error!("Failed to check status: {}", e);
                        return Err(anyhow::anyhow!(e));
                    }
                }
            }
        }
        
        Commands::Agent { message, model } => {
            let Some(msg) = message else {
                println!("Usage: oclaws agent --message \"your prompt\" [--model gpt-4]");
                return Ok(());
            };
            let client = reqwest::Client::new();
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": msg}],
                "stream": false
            });
            match client.post(format!("{}/v1/chat/completions", cli.gateway_url))
                .json(&body).send().await
            {
                Ok(resp) if resp.status().is_success() => {
                    let json: serde_json::Value = resp.json().await.unwrap_or_default();
                    if let Some(text) = json["choices"][0]["message"]["content"].as_str() {
                        println!("{}", text);
                    } else {
                        println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    error!("Request failed ({}): {}", status, text);
                }
                Err(e) => error!("Failed to connect to gateway: {}", e),
            }
        }

        Commands::Sessions { action } => {
            let client = reqwest::Client::new();
            let base = &cli.gateway_url;
            match action {
                SessionAction::List => {
                    match client.get(format!("{}/sessions", base)).send().await {
                        Ok(resp) => println!("{}", resp.text().await.unwrap_or_default()),
                        Err(e) => error!("Failed to list sessions: {}", e),
                    }
                }
                SessionAction::Show { key } => {
                    match client.get(format!("{}/sessions", base)).send().await {
                        Ok(resp) => {
                            let json: serde_json::Value = resp.json().await.unwrap_or_default();
                            let sessions = json["sessions"].as_array();
                            let found = sessions.and_then(|arr| {
                                arr.iter().find(|s| s["key"].as_str() == Some(&key))
                            });
                            match found {
                                Some(s) => println!("{}", serde_json::to_string_pretty(s).unwrap_or_default()),
                                None => println!("Session '{}' not found", key),
                            }
                        }
                        Err(e) => error!("Failed to get session: {}", e),
                    }
                }
                SessionAction::Delete { key } => {
                    match client.delete(format!("{}/sessions/{}", base, key)).send().await {
                        Ok(resp) => println!("{}", resp.text().await.unwrap_or_default()),
                        Err(e) => error!("Failed to delete session: {}", e),
                    }
                }
            }
        }

        Commands::Models { action } => {
            match action {
                ModelAction::List => {
                    let client = reqwest::Client::new();
                    match client.get(format!("{}/models", cli.gateway_url)).send().await {
                        Ok(resp) => println!("{}", resp.text().await.unwrap_or_default()),
                        Err(e) => error!("Failed to list models: {}", e),
                    }
                }
            }
        }

        Commands::Message { session, text } => {
            let sid = session.unwrap_or_else(|| "default".into());
            let client = reqwest::Client::new();
            let body = serde_json::json!({
                "model": "default",
                "messages": [{"role": "user", "content": text}],
                "stream": false
            });
            match client.post(format!("{}/v1/chat/completions", cli.gateway_url))
                .header("X-Session-Id", &sid)
                .json(&body).send().await
            {
                Ok(resp) if resp.status().is_success() => {
                    let json: serde_json::Value = resp.json().await.unwrap_or_default();
                    if let Some(content) = json["choices"][0]["message"]["content"].as_str() {
                        println!("{}", content);
                    } else {
                        println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    error!("Request failed ({}): {}", status, text);
                }
                Err(e) => error!("Failed to connect to gateway: {}", e),
            }
        }

        Commands::Status => {
            let client = reqwest::Client::new();
            match client.get(format!("{}/health", cli.gateway_url)).send().await {
                Ok(resp) => println!("{}", resp.text().await.unwrap_or_default()),
                Err(_) => println!("Gateway is not running."),
            }
        }

        Commands::Version => {
            println!("oclaws {}", env!("CARGO_PKG_VERSION"));
            println!("Edition: 2024");
        }

        Commands::Tui => {
            let config_path = cli.config.as_ref().map(std::path::PathBuf::from)
                .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
            let mut manager = ConfigManager::new(config_path);
            let gateway_url = if manager.load().is_ok() {
                manager.config_mut().apply_env_overrides();
                let gw = manager.config().gateway.as_ref();
                let port = gw.and_then(|g| g.port).unwrap_or(8080) as u16;
                let bind = gw.and_then(|g| g.bind.clone()).unwrap_or_else(|| "127.0.0.1".to_string());
                format!("http://{}:{}", bind, port + 1)
            } else {
                cli.gateway_url.clone()
            };
            let tui_config = oclaws_tui_core::TuiConfig {
                gateway_url,
                ..Default::default()
            };
            let mut app = oclaws_tui_core::TuiApp::new(tui_config);
            if let Err(e) = app.run().await {
                error!("TUI error: {}", e);
                return Err(anyhow::anyhow!(e));
            }
        }

        Commands::Daemon { action } => {
            let manager = oclaws_daemon_core::ServiceManager::new();
            match action {
                DaemonAction::Start { port } => {
                    let exe = std::env::current_exe().unwrap_or_default();
                    let config = oclaws_daemon_core::ServiceConfig::new("oclaws-gateway", exe);
                    let service = oclaws_daemon_core::DaemonService::new(config);
                    manager.register(service).await.map_err(|e| anyhow::anyhow!("{}", e))?;
                    manager.start("oclaws-gateway").await.map_err(|e| anyhow::anyhow!("{}", e))?;
                    info!("Daemon started on port {}", port);
                }
                DaemonAction::Stop => {
                    manager.stop("oclaws-gateway").await.map_err(|e| anyhow::anyhow!("{}", e))?;
                    info!("Daemon stopped");
                }
                DaemonAction::Status => {
                    match manager.status("oclaws-gateway").await {
                        Ok((state, pid)) => println!("State: {:?}, PID: {:?}", state, pid),
                        Err(e) => println!("Not running: {}", e),
                    }
                }
            }
        }

        Commands::Plugin { action } => {
            let loader = oclaws_plugin_core::PluginLoader::new();
            match action {
                PluginAction::List => {
                    let plugins = loader.discover_plugins();
                    if plugins.is_empty() {
                        println!("No plugins found.");
                    } else {
                        for p in &plugins {
                            println!("  {}  v{}  {}", p.manifest.id, p.manifest.version,
                                p.manifest.description.as_deref().unwrap_or(""));
                        }
                    }
                }
                PluginAction::Info { id } => {
                    let plugins = loader.discover_plugins();
                    match plugins.iter().find(|p| p.manifest.id == id) {
                        Some(p) => {
                            let m = &p.manifest;
                            println!("ID: {}", m.id);
                            println!("Name: {}", m.name);
                            println!("Version: {}", m.version);
                            if let Some(d) = &m.description { println!("Description: {}", d); }
                            if let Some(k) = &m.kind { println!("Kind: {}", k); }
                            if let Some(path) = &p.file_path { println!("Path: {}", path); }
                            if !m.capabilities.is_empty() { println!("Capabilities: {}", m.capabilities.join(", ")); }
                            if !m.tags.is_empty() { println!("Tags: {}", m.tags.join(", ")); }
                        }
                        None => println!("Plugin '{}' not found", id),
                    }
                }
                PluginAction::Enable { id } => {
                    let config_path = cli.config.map(std::path::PathBuf::from)
                        .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
                    let mut manager = ConfigManager::new(config_path);
                    if manager.load().is_ok() {
                        let cfg = manager.config_mut();
                        let plugins = cfg.plugins.get_or_insert_with(Default::default);
                        let entry = plugins.entries.entry(id.clone()).or_default();
                        entry.enabled = Some(true);
                        if let Err(e) = manager.save() {
                            error!("Failed to save config: {}", e);
                        } else {
                            println!("Plugin '{}' enabled", id);
                        }
                    }
                }
                PluginAction::Disable { id } => {
                    let config_path = cli.config.map(std::path::PathBuf::from)
                        .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
                    let mut manager = ConfigManager::new(config_path);
                    if manager.load().is_ok() {
                        let cfg = manager.config_mut();
                        let plugins = cfg.plugins.get_or_insert_with(Default::default);
                        let entry = plugins.entries.entry(id.clone()).or_default();
                        entry.enabled = Some(false);
                        if let Err(e) = manager.save() {
                            error!("Failed to save config: {}", e);
                        } else {
                            println!("Plugin '{}' disabled", id);
                        }
                    }
                }
            }
        }
    }
    
    Ok(())
}

fn create_llm_provider(config: &Config) -> Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>> {
    let models = config.models.as_ref()?;
    let providers = models.providers.as_ref()?;
    let (name, p) = providers.iter().next()?;
    let provider_type = p.provider.parse::<oclaws_llm_core::providers::ProviderType>().ok()?;
    let defaults = oclaws_llm_core::providers::ProviderDefaults {
        model: p.model.clone(),
        max_tokens: p.max_tokens,
        temperature: p.temperature,
        headers: p.headers.clone(),
    };
    match oclaws_llm_core::providers::LlmFactory::create(
        provider_type,
        p.api_key.as_deref().unwrap_or(""),
        p.base_url.as_deref(),
        defaults,
    ) {
        Ok(provider) => {
            info!("LLM provider '{}' ({}) initialized", name, p.provider);
            Some(Arc::from(provider))
        }
        Err(e) => {
            error!("Failed to create LLM provider '{}': {}", name, e);
            None
        }
    }
}

struct HttpServerParams {
    port: u16,
    gateway: Gateway,
    gateway_server: Arc<GatewayServer>,
    llm_provider: Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>,
    channel_manager: Option<Arc<RwLock<oclaws_channel_core::ChannelManager>>>,
    tool_registry: Arc<oclaws_tools_core::tool::ToolRegistry>,
    plugin_registrations: Option<Arc<oclaws_plugin_core::PluginRegistrations>>,
    cron_service: Option<Arc<oclaws_cron_core::CronService>>,
    full_config: oclaws_config::Config,
    config_path: std::path::PathBuf,
}

async fn build_http_server(p: HttpServerParams) -> anyhow::Result<oclaws_gateway_core::HttpServer> {
    let HttpServerParams {
        port, gateway, gateway_server, llm_provider, channel_manager,
        tool_registry, plugin_registrations, cron_service, full_config, config_path,
    } = p;
    use oclaws_gateway_core::create_http_server;
    let mut server = create_http_server(port, gateway, gateway_server).await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    server = server.with_full_config(full_config, config_path);
    server = server.with_tool_registry(tool_registry);
    if let Some(regs) = plugin_registrations {
        server = server.with_plugin_registrations(regs);
    }
    if let Some(cron) = cron_service {
        server = server.with_cron_service(cron);
    }
    if let Some(provider) = llm_provider {
        server = server.with_llm_provider(provider);
    }
    if let Some(cm) = channel_manager {
        server = server.with_channel_manager(cm);
    }
    Ok(server)
}

async fn telegram_poll_loop(
    bot_token: String,
    llm_provider: Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>,
    channel_manager: Option<Arc<RwLock<oclaws_channel_core::ChannelManager>>>,
    tool_registry: Arc<oclaws_tools_core::tool::ToolRegistry>,
) {
    let client = reqwest::Client::new();
    let base = format!("https://api.telegram.org/bot{}", bot_token);

    // Delete any existing webhook to enable polling
    let _ = client.post(format!("{}/deleteWebhook", base)).send().await;
    info!("Telegram polling started");

    let mut offset: i64 = 0;
    loop {
        let resp = client
            .get(format!("{}/getUpdates?offset={}&timeout=30", base, offset))
            .timeout(std::time::Duration::from_secs(35))
            .send()
            .await;

        let body = match resp {
            Ok(r) => r.json::<serde_json::Value>().await.unwrap_or_default(),
            Err(e) => {
                error!("Telegram poll error: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };

        let updates = match body["result"].as_array() {
            Some(arr) => arr.clone(),
            None => continue,
        };

        for update in updates {
            if let Some(uid) = update["update_id"].as_i64() {
                offset = uid + 1;
            }

            let msg = update.get("message").or_else(|| update.get("edited_message"));
            let Some(msg) = msg else { continue };

            let Some(chat_id) = msg.pointer("/chat/id").and_then(|v| v.as_i64()) else { continue };
            let msg_id = msg.get("message_id").and_then(|v| v.as_i64());

            // Extract user input from various message types
            let text = if let Some(t) = msg.get("text").and_then(|v| v.as_str()) {
                t.to_string()
            } else if let Some(caption) = msg.get("caption").and_then(|v| v.as_str()) {
                // Photo/video/document with caption
                caption.to_string()
            } else if msg.get("voice").is_some() || msg.get("audio").is_some() {
                "[用户发送了一条语音/音频消息]".to_string()
            } else if msg.get("photo").is_some() {
                "[用户发送了一张图片]".to_string()
            } else if msg.get("video").is_some() {
                "[用户发送了一个视频]".to_string()
            } else if msg.get("sticker").is_some() {
                let emoji = msg.pointer("/sticker/emoji").and_then(|v| v.as_str()).unwrap_or("🙂");
                format!("[用户发送了贴纸: {}]", emoji)
            } else if msg.get("document").is_some() {
                let name = msg.pointer("/document/file_name").and_then(|v| v.as_str()).unwrap_or("file");
                format!("[用户发送了文件: {}]", name)
            } else {
                continue;
            };

            // Send typing indicator
            let _ = client.post(format!("{}/sendChatAction", base))
                .json(&serde_json::json!({"chat_id": chat_id, "action": "typing"}))
                .send().await;

            // Call Agent with tools (session = telegram_{chat_id})
            let Some(ref provider) = llm_provider else { continue };
            let session_key = format!("telegram_{}", chat_id);
            let reply = match run_agent(provider, &tool_registry, &text, Some(&session_key)).await {
                Ok(r) => r,
                Err(e) => {
                    error!("Agent error for Telegram: {}", e);
                    continue;
                }
            };

            // Send reply via channel manager (with reply_to + long msg split)
            let sent = if let Some(ref cm) = channel_manager {
                let mgr = cm.read().await;
                if let Some(ch) = mgr.get("telegram").await {
                    let mut meta = std::collections::HashMap::new();
                    meta.insert("chat_id".to_string(), chat_id.to_string());
                    if let Some(mid) = msg_id {
                        meta.insert("reply_to_message_id".to_string(), mid.to_string());
                    }
                    let m = oclaws_channel_core::traits::ChannelMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        channel: "telegram".to_string(),
                        sender: "bot".to_string(),
                        content: reply.clone(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default().as_millis() as i64,
                        metadata: meta,
                    };
                    ch.read().await.send_message(&m).await.is_ok()
                } else { false }
            } else { false };

            if !sent {
                let mut body = serde_json::json!({"chat_id": chat_id, "text": reply});
                if let Some(mid) = msg_id { body["reply_to_message_id"] = mid.into(); }
                let _ = client.post(format!("{}/sendMessage", base))
                    .json(&body).send().await;
            }

            info!("Replied to Telegram chat {}", chat_id);
        }
    }
}

/// Bridge: wraps ToolRegistry as agent-core's ToolExecutor, then runs Agent.
struct ToolRegistryExecutor(Arc<oclaws_tools_core::tool::ToolRegistry>);

#[async_trait::async_trait]
impl oclaws_agent_core::agent::ToolExecutor for ToolRegistryExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> Result<String, String> {
        let args: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_default();
        let call = oclaws_tools_core::tool::ToolCall {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            arguments: args,
        };
        let resp = self.0.execute_call(call).await;
        match resp.error {
            Some(err) => Err(err),
            None => Ok(serde_json::to_string(&resp.result).unwrap_or_default()),
        }
    }

    fn available_tools(&self) -> Vec<oclaws_llm_core::chat::Tool> {
        self.0.list_for_llm().into_iter().filter_map(|v| {
            Some(oclaws_llm_core::chat::Tool {
                type_: "function".into(),
                function: oclaws_llm_core::chat::ToolFunction {
                    name: v["name"].as_str()?.to_string(),
                    description: v["description"].as_str()?.to_string(),
                    parameters: v["parameters"].clone(),
                },
            })
        }).collect()
    }
}

fn build_system_prompt(registry: &oclaws_tools_core::tool::ToolRegistry) -> String {
    let tool_lines: Vec<String> = registry.list_for_llm().iter()
        .filter_map(|v| {
            let name = v["name"].as_str()?;
            let desc = v["description"].as_str().unwrap_or("");
            Some(format!("- {}: {}", name, desc))
        })
        .collect();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!(
        "You are a helpful AI assistant.\n\n\
         ## Available Tools\n{}\n\n\
         You CAN access the internet. Use web_fetch for APIs/simple pages, browse for JS-heavy sites.\n\n\
         ## Runtime\n\
         Current time: {}\nOS: {} ({})\nWorking directory: {}\n\n\
         Respond in the user's language.",
        tool_lines.join("\n"), now, os, arch, cwd
    )
}

async fn run_agent(
    provider: &Arc<dyn oclaws_llm_core::providers::LlmProvider>,
    registry: &Arc<oclaws_tools_core::tool::ToolRegistry>,
    input: &str,
    session_id: Option<&str>,
) -> Result<String, String> {
    use oclaws_agent_core::agent::{Agent, AgentConfig};
    let model = provider.default_model().to_string();
    let prompt = build_system_prompt(registry);
    let config = AgentConfig::new("channel-agent", &model, "default")
        .with_system_prompt(&prompt);
    let mut agent = Agent::new(config, provider.clone());
    if let Some(sid) = session_id {
        agent = agent.with_transcript(sid);
    }
    agent.initialize().await.map_err(|e| e.to_string())?;
    let executor = ToolRegistryExecutor(registry.clone());
    agent.run_with_tools(input, &executor).await.map_err(|e| e.to_string())
}

async fn feishu_ws_loop(
    app_id: String,
    app_secret: String,
    llm_provider: Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>,
    channel_manager: Option<Arc<RwLock<oclaws_channel_core::ChannelManager>>>,
    tool_registry: Arc<oclaws_tools_core::tool::ToolRegistry>,
) {
    use futures::stream::StreamExt;
    use futures::SinkExt;
    use prost::Message;
    use tokio_tungstenite::tungstenite::Message as WsMsg;

    let client = reqwest::Client::new();
    info!("Feishu WebSocket long connection starting");

    loop {
        let ws_url = match feishu_get_ws_endpoint(&client, &app_id, &app_secret).await {
            Ok(u) => u,
            Err(e) => {
                error!("Feishu WS endpoint error: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        info!("Feishu WS connecting to: {}", ws_url);

        let ws_stream = match tokio_tungstenite::connect_async(&ws_url).await {
            Ok((stream, _)) => stream,
            Err(e) => {
                error!("Feishu WS connect error: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        info!("Feishu WebSocket connected");
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        // Ping loop via channel to avoid mutex
        let (ping_tx, mut ping_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
        let ping_sender = ping_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(90)).await;
                let frame = fs_ping_frame();
                if ping_sender.send(frame.encode_to_vec()).await.is_err() { break; }
                debug!("Feishu ping queued");
            }
        });

        // Writer task: forwards ping/response frames to WS
        let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
        let write_tx2 = write_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(buf) = ping_rx.recv() => {
                        if ws_tx.send(WsMsg::Binary(buf.into())).await.is_err() { break; }
                    }
                    Some(buf) = write_rx.recv() => {
                        if ws_tx.send(WsMsg::Binary(buf.into())).await.is_err() { break; }
                    }
                    else => break,
                }
            }
        });

        let combine_cache: std::collections::HashMap<String, Vec<Option<Vec<u8>>>> = std::collections::HashMap::new();
        let combine_cache = Arc::new(tokio::sync::Mutex::new(combine_cache));

        debug!("Feishu WS receive loop started");
        while let Some(msg) = ws_rx.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => { error!("Feishu WS read error: {}", e); break; }
            };
            debug!("Feishu WS recv: {:?}", msg);

            let data = match msg {
                WsMsg::Binary(b) => b,
                WsMsg::Text(t) => t.as_bytes().to_vec().into(),
                WsMsg::Ping(d) => {
                    let _ = write_tx2.send(d.to_vec()).await; // pong
                    continue;
                }
                WsMsg::Pong(_) => continue,
                WsMsg::Close(c) => { info!("Feishu WS closed: {:?}", c); break; }
                _ => continue,
            };

            let frame = match FsFrame::decode(&data[..]) {
                Ok(f) => f,
                Err(e) => { error!("Feishu frame decode error: {}", e); continue; }
            };

            match frame.method {
                0 => {
                    let t = fs_header(&frame.headers, "type");
                    debug!("Feishu control frame: type={}", t);
                }
                1 => {
                    let msg_type = fs_header(&frame.headers, "type");
                    let msg_id = fs_header(&frame.headers, "message_id");
                    let sum: usize = fs_header(&frame.headers, "sum").parse().unwrap_or(1);
                    let seq: usize = fs_header(&frame.headers, "seq").parse().unwrap_or(0);

                    let payload = if sum > 1 {
                        let mut cache = combine_cache.lock().await;
                        let entry = cache.entry(msg_id.clone()).or_insert_with(|| vec![None; sum]);
                        if seq < entry.len() { entry[seq] = Some(frame.payload.clone()); }
                        if entry.iter().all(|p| p.is_some()) {
                            let combined: Vec<u8> = entry.iter().flat_map(|p| p.as_ref().unwrap().clone()).collect();
                            cache.remove(&msg_id);
                            combined
                        } else { continue; }
                    } else {
                        frame.payload.clone()
                    };

                    let resp = fs_response_frame(&frame, 200);
                    let _ = write_tx2.send(resp.encode_to_vec()).await;

                    if msg_type == "event" {
                        let provider = llm_provider.clone();
                        let cm = channel_manager.clone();
                        let tr = tool_registry.clone();
                        tokio::spawn(async move {
                            if let Err(e) = feishu_handle_event(provider, cm, tr, &payload).await {
                                error!("Feishu event error: {}", e);
                            }
                        });
                    } else {
                        debug!("Feishu ignoring frame type: {}", msg_type);
                    }
                }
                _ => {}
            }
        }

        warn!("Feishu WS disconnected, reconnecting...");
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

async fn feishu_get_ws_endpoint(client: &reqwest::Client, app_id: &str, app_secret: &str) -> Result<String, String> {
    let body = serde_json::json!({"AppID": app_id, "AppSecret": app_secret});
    let resp = client.post("https://open.feishu.cn/callback/ws/endpoint")
        .header("locale", "zh")
        .json(&body)
        .send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    info!("Feishu WS endpoint response ({}): {}", status, body);
    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("JSON parse error: {} body: {}", e, body))?;
    json["data"]["URL"].as_str()
        .or_else(|| json["data"]["url"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("No WS URL in response: {}", json))
}

fn fs_header(headers: &[FsHeader], key: &str) -> String {
    headers.iter().find(|h| h.key == key).map(|h| h.value.clone()).unwrap_or_default()
}

fn fs_ping_frame() -> FsFrame {
    FsFrame {
        method: 0, // control
        headers: vec![FsHeader { key: "type".into(), value: "ping".into() }],
        ..Default::default()
    }
}

fn fs_response_frame(req: &FsFrame, code: i32) -> FsFrame {
    let resp = serde_json::json!({"code": code, "headers": {}, "data": ""});
    let mut headers = req.headers.clone();
    // Add biz_rt header
    headers.push(FsHeader { key: "biz_rt".into(), value: "0".into() });
    FsFrame {
        seq_id: req.seq_id,
        log_id: req.log_id,
        service: req.service,
        method: req.method,
        headers,
        payload: resp.to_string().into_bytes(),
        log_id_new: req.log_id_new.clone(),
        ..Default::default()
    }
}

async fn feishu_handle_event(
    llm_provider: Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>,
    channel_manager: Option<Arc<RwLock<oclaws_channel_core::ChannelManager>>>,
    tool_registry: Arc<oclaws_tools_core::tool::ToolRegistry>,
    payload: &[u8],
) -> Result<(), String> {
    let event: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|e| format!("Event JSON parse error: {}", e))?;

    let event_type = event.pointer("/header/event_type")
        .and_then(|v| v.as_str()).unwrap_or("");

    debug!("Feishu event payload: {}", event);
    info!("Feishu event: {}", event_type);

    match event_type {
        // ── IM: 消息 ──
        "im.message.receive_v1" => {
            feishu_on_message_receive(&event, &llm_provider, &channel_manager, &tool_registry).await?;
        }
        "im.message.message_read_v1" => {
            let reader = event.pointer("/event/reader/reader_id/open_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu message read by {}", reader);
        }
        "im.message.recalled_v1" => {
            let mid = event.pointer("/event/message_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu message recalled: {}", mid);
        }
        "im.message.reaction.created_v1" => {
            let emoji = event.pointer("/event/reaction_type/emoji_type").and_then(|v| v.as_str()).unwrap_or("?");
            let mid = event.pointer("/event/message_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu reaction added: {} on {}", emoji, mid);
        }
        "im.message.reaction.deleted_v1" => {
            let emoji = event.pointer("/event/reaction_type/emoji_type").and_then(|v| v.as_str()).unwrap_or("?");
            let mid = event.pointer("/event/message_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu reaction removed: {} on {}", emoji, mid);
        }

        // ── IM: 群组 ──
        "im.chat.created_v1" => {
            let name = event.pointer("/event/name").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu chat created: {}", name);
        }
        "im.chat.disbanded_v1" => {
            let cid = event.pointer("/event/chat_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu chat disbanded: {}", cid);
        }
        "im.chat.updated_v1" => {
            let cid = event.pointer("/event/chat_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu chat updated: {}", cid);
        }

        // ── IM: 群成员 ──
        "im.chat.member.bot.added_v1" => {
            let cid = event.pointer("/event/chat_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Bot added to Feishu chat: {}", cid);
            // 可选：入群自动打招呼
            if let Some(ref cm) = channel_manager {
                let mgr = cm.read().await;
                if let Some(ch) = mgr.get("feishu").await {
                    let mut meta = std::collections::HashMap::new();
                    meta.insert("chat_id".to_string(), cid.to_string());
                    let msg = oclaws_channel_core::traits::ChannelMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        channel: "feishu".into(), sender: "bot".into(),
                        content: "Hello! I'm your AI assistant. Send me a message to get started.".into(),
                        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64,
                        metadata: meta,
                    };
                    let _ = ch.read().await.send_message(&msg).await;
                }
            }
        }
        "im.chat.member.bot.deleted_v1" => {
            let cid = event.pointer("/event/chat_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Bot removed from Feishu chat: {}", cid);
        }
        "im.chat.member.user.added_v1" => {
            let cid = event.pointer("/event/chat_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("User added to Feishu chat: {}", cid);
        }
        "im.chat.member.user.deleted_v1" => {
            let cid = event.pointer("/event/chat_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("User removed from Feishu chat: {}", cid);
        }
        "im.chat.member.user.withdrawn_v1" => {
            let cid = event.pointer("/event/chat_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("User join withdrawn in Feishu chat: {}", cid);
        }

        // ── IM: 会话标签页/置顶/公告 ──
        "im.chat.access_event.updated_v1" => {
            info!("Feishu chat access event updated");
        }

        // ── 通讯录 ──
        "contact.user.created_v3" => {
            let name = event.pointer("/event/object/name").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu contact user created: {}", name);
        }
        "contact.user.deleted_v3" => {
            let uid = event.pointer("/event/object/open_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu contact user deleted: {}", uid);
        }
        "contact.user.updated_v3" => {
            let uid = event.pointer("/event/object/open_id").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu contact user updated: {}", uid);
        }
        "contact.department.created_v3" => {
            let name = event.pointer("/event/object/name").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu department created: {}", name);
        }
        "contact.department.deleted_v3" => {
            info!("Feishu department deleted");
        }
        "contact.department.updated_v3" => {
            info!("Feishu department updated");
        }
        "contact.employee_type_enum.created_v3"
        | "contact.employee_type_enum.updated_v3"
        | "contact.employee_type_enum.deleted_v3" => {
            info!("Feishu employee type enum changed: {}", event_type);
        }
        "contact.scope.updated_v3" => {
            info!("Feishu contact scope updated");
        }

        // ── 应用 ──
        "application.bot.menu_v6" => {
            let key = event.pointer("/event/event_key").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu bot menu clicked: {}", key);
        }
        "application.feedback.created_v6" | "application.feedback.updated_v6" => {
            info!("Feishu app feedback: {}", event_type);
        }
        "application.application.visibility.added_v6" => {
            info!("Feishu app visibility added");
        }
        "application.application.created_v6" => {
            info!("Feishu app created");
        }

        // ── 审批 ──
        "approval.approval.updated_v4" => {
            info!("Feishu approval definition updated");
        }
        "approval.instance.status_changed_v4" => {
            let status = event.pointer("/event/status").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu approval instance status: {}", status);
        }
        "approval.task.updated_v4" => {
            let status = event.pointer("/event/status").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu approval task: {}", status);
        }
        "approval.leave.approval_v4" | "approval.trip.approval_v4"
        | "approval.remedy.approval_v4" | "approval.shift.approval_v4"
        | "approval.overtime.approval_v4" => {
            info!("Feishu attendance approval: {}", event_type);
        }

        // ── 日历 ──
        "calendar.calendar.changed_v4" => {
            info!("Feishu calendar changed");
        }
        "calendar.calendar.event.changed_v4" => {
            info!("Feishu calendar event changed");
        }
        "calendar.calendar.acl.created_v4" | "calendar.calendar.acl.deleted_v4" => {
            info!("Feishu calendar ACL: {}", event_type);
        }

        // ── 云文档 ──
        "drive.file.read_v1" => {
            info!("Feishu drive file read");
        }
        "drive.file.edit_v1" => {
            info!("Feishu drive file edited");
        }
        "drive.file.title_updated_v1" => {
            info!("Feishu drive file title updated");
        }
        "drive.file.permission_member_added_v1" | "drive.file.permission_member_removed_v1" => {
            info!("Feishu drive permission: {}", event_type);
        }
        "drive.file.trashed_v1" | "drive.file.deleted_v1" => {
            info!("Feishu drive file removed: {}", event_type);
        }
        "drive.file.bitable_field_changed_v1" | "drive.file.bitable_record_changed_v1" => {
            info!("Feishu bitable changed: {}", event_type);
        }

        // ── 视频会议 ──
        "vc.meeting.meeting_started_v1" | "vc.meeting.meeting_ended_v1" => {
            let topic = event.pointer("/event/meeting/topic").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Feishu meeting {}: {}", event_type, topic);
        }
        "vc.meeting.join_meeting_v1" | "vc.meeting.leave_meeting_v1" => {
            info!("Feishu meeting participant: {}", event_type);
        }
        "vc.meeting.recording_started_v1" | "vc.meeting.recording_ended_v1"
        | "vc.meeting.recording_ready_v1" => {
            info!("Feishu meeting recording: {}", event_type);
        }
        "vc.meeting.share_started_v1" | "vc.meeting.share_ended_v1" => {
            info!("Feishu meeting share: {}", event_type);
        }

        // ── 服务台 ──
        "helpdesk.ticket.created_v1" | "helpdesk.ticket.updated_v1" => {
            info!("Feishu helpdesk ticket: {}", event_type);
        }
        "helpdesk.ticket_message.created_v1" => {
            info!("Feishu helpdesk ticket message");
        }
        "helpdesk.notification.approve_v1" => {
            info!("Feishu helpdesk notification approve");
        }

        // ── 任务 ──
        "task.task.update_tenant_v1" | "task.task.updated_v1" => {
            info!("Feishu task updated");
        }
        "task.task.comment_updated_v1" => {
            info!("Feishu task comment updated");
        }

        // ── 考勤 ──
        "attendance.user_flow.created_v1" => {
            info!("Feishu attendance flow created");
        }
        "attendance.user_task.updated_v1" => {
            info!("Feishu attendance task updated");
        }

        // ── 招聘 ──
        "hire.application.stage_changed_v1" | "hire.ehr_import_task.for_internship_offer_imported_v1" => {
            info!("Feishu hire: {}", event_type);
        }

        // ── 飞书人事 ──
        "corehr.employment.created_v1" | "corehr.employment.updated_v1"
        | "corehr.employment.deleted_v1" | "corehr.employment.converted_v1" => {
            info!("Feishu corehr employment: {}", event_type);
        }

        _ => {
            info!("Unhandled Feishu event: {} payload_size={}", event_type, payload.len());
        }
    }

    Ok(())
}

async fn feishu_on_message_receive(
    event: &serde_json::Value,
    llm_provider: &Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>,
    channel_manager: &Option<Arc<RwLock<oclaws_channel_core::ChannelManager>>>,
    tool_registry: &Arc<oclaws_tools_core::tool::ToolRegistry>,
) -> Result<(), String> {
    let chat_id = event.pointer("/event/message/chat_id")
        .and_then(|v| v.as_str()).unwrap_or("").to_string();
    let message_id = event.pointer("/event/message/message_id")
        .and_then(|v| v.as_str()).unwrap_or("").to_string();
    let msg_type = event.pointer("/event/message/message_type")
        .and_then(|v| v.as_str()).unwrap_or("text");

    info!("Feishu msg: chat_id={}, message_id={}, type={}", chat_id, message_id, msg_type);

    let text = match msg_type {
        "text" => {
            let content_str = event.pointer("/event/message/content")
                .and_then(|v| v.as_str()).unwrap_or("{}");
            let content: serde_json::Value = serde_json::from_str(content_str).unwrap_or_default();
            content["text"].as_str().unwrap_or("").to_string()
        }
        other => format!("[飞书{}消息]", other),
    };

    info!("Feishu text: {}", text);
    if text.is_empty() { return Ok(()); }

    let Some(provider) = llm_provider else {
        warn!("Feishu: no LLM provider configured");
        return Ok(());
    };

    // Send "thinking" placeholder first
    let placeholder_mid = if let Some(cm) = channel_manager {
        let mgr = cm.read().await;
        if let Some(ch) = mgr.get("feishu").await {
            let mut meta = std::collections::HashMap::new();
            meta.insert("chat_id".to_string(), chat_id.clone());
            meta.insert("message_id".to_string(), message_id.clone());
            let msg = oclaws_channel_core::traits::ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "feishu".into(), sender: "bot".into(),
                content: "正在思考...".into(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default().as_millis() as i64,
                metadata: meta,
            };
            ch.read().await.send_message(&msg).await.ok()
        } else { None }
    } else { None };

    info!("Feishu: calling agent...");
    let session_key = format!("feishu_{}", chat_id);
    let reply = run_agent(provider, tool_registry, &text, Some(&session_key)).await?;
    info!("Feishu: agent reply len={}", reply.len());

    // Update placeholder with actual reply, or send new message
    if let Some(cm) = channel_manager {
        let mgr = cm.read().await;
        if let Some(ch) = mgr.get("feishu").await {
            let mut meta = std::collections::HashMap::new();
            meta.insert("chat_id".to_string(), chat_id.clone());
            if let Some(ref mid) = placeholder_mid {
                meta.insert("update_message_id".to_string(), mid.clone());
            } else {
                meta.insert("message_id".to_string(), message_id.clone());
            }
            let msg = oclaws_channel_core::traits::ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "feishu".into(), sender: "bot".into(),
                content: reply.clone(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default().as_millis() as i64,
                metadata: meta,
            };
            match ch.read().await.send_message(&msg).await {
                Ok(mid) => info!("Feishu reply sent, mid={}", mid),
                Err(e) => error!("Feishu send_message failed: {:?}", e),
            }
        }
    }

    Ok(())
}
