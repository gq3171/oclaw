use clap::{Parser, Subcommand};
use oclaws_gateway_core::GatewayServer;
use oclaws_config::{Config, ConfigManager};
use oclaws_config::settings::Gateway;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, Level};
use tracing_subscriber::{FmtSubscriber, fmt};

mod wizard;

#[derive(Parser)]
#[command(name = "oclaws")]
#[command(version = "0.1.0")]
#[command(about = "OCLAWS - Open CLAW System", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    #[arg(short, long, default_value = "info")]
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
        #[arg(short, long, default_value = "8080")]
        port: u16,
        
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
enum SkillAction {
    Setup,
    List,
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
    
    match cli.command {
        Commands::Start { port, host, http_only, ws_only } => {
            // 1. Load config
            let config_path = cli.config
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
            let mut manager = ConfigManager::new(config_path.clone());
            let config = if manager.load().is_ok() {
                info!("Loaded config from {:?}", config_path);
                manager.config().clone()
            } else {
                warn!("No config found, using defaults");
                Config::default()
            };

            // 2. Build gateway config with CLI overrides
            let bind_addr = host.as_deref().unwrap_or("0.0.0.0");
            let mut gateway_config = config.gateway.clone().unwrap_or_default();
            gateway_config.port = Some(port as i32);
            gateway_config.bind = Some(bind_addr.to_string());

            // 3. Create LLM provider from config
            let llm_provider = create_llm_provider(&config);

            // 4. Create channel manager from config
            let channel_manager = if let Some(ref channels) = config.channels {
                let cm = oclaws_channel_core::ChannelManager::from_config(channels).await;
                info!("Channel manager initialized");
                Some(Arc::new(RwLock::new(cm)))
            } else {
                None
            };

            // 5. Start config watcher
            let watcher = oclaws_config::ConfigWatcher::new(config_path);
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
                let server = build_http_server(port, gateway_config, gateway_server, llm_provider, channel_manager).await?;
                if let Err(e) = server.start().await {
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

                let server = build_http_server(port + 1, gateway_config, gateway_server, llm_provider, channel_manager).await?;
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
            use wizard::SkillWizard;
            
            match action {
                SkillAction::Setup => {
                    if let Err(e) = SkillWizard::run() {
                        tracing::error!("Skill setup failed: {}", e);
                        return Err(anyhow::anyhow!(e));
                    }
                }
                SkillAction::List => {
                    tracing::info!("Available skills:");
                    tracing::info!("Use 'oclaws skill setup' to configure skills.");
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
            let config = oclaws_tui_core::TuiConfig::default();
            let mut app = oclaws_tui_core::TuiApp::new(config);
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
    }
    
    Ok(())
}

fn create_llm_provider(config: &Config) -> Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>> {
    let models = config.models.as_ref()?;
    let providers = models.providers.as_ref()?;
    let (name, p) = providers.iter().next()?;
    let provider_type = p.provider.parse::<oclaws_llm_core::providers::ProviderType>().ok()?;
    match oclaws_llm_core::providers::LlmFactory::create(
        provider_type,
        p.api_key.as_deref().unwrap_or(""),
        p.base_url.as_deref(),
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

async fn build_http_server(
    port: u16,
    gateway: Gateway,
    gateway_server: Arc<GatewayServer>,
    llm_provider: Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>,
    channel_manager: Option<Arc<RwLock<oclaws_channel_core::ChannelManager>>>,
) -> anyhow::Result<oclaws_gateway_core::HttpServer> {
    use oclaws_gateway_core::create_http_server;
    let mut server = create_http_server(port, gateway, gateway_server).await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    if let Some(provider) = llm_provider {
        server = server.with_llm_provider(provider);
    }
    if let Some(cm) = channel_manager {
        server = server.with_channel_manager(cm);
    }
    Ok(server)
}
