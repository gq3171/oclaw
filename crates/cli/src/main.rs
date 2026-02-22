use clap::{Parser, Subcommand};
use oclaws_gateway_core::GatewayServer;
use oclaws_config::{Config, ConfigManager};
use oclaws_config::settings::Gateway;
use std::sync::Arc;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;

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
    
    #[arg(short, long)]
    config: Option<String>,
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
    },
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },
    Version,
}

#[derive(Subcommand)]
enum ConfigAction {
    Init,
    Show,
    Validate,
}

#[derive(Subcommand)]
enum ProviderAction {
    Setup,
    Status,
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
    
    FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .init();
    
    match cli.command {
        Commands::Start { port, host: _, http_only, ws_only } => {
            let gateway_config = Gateway {
                port: Some(port as i32),
                mode: Some("server".to_string()),
                bind: Some("0.0.0.0".to_string()),
                ..Default::default()
            };
            
            let gateway = Arc::new(gateway_config);
            let gateway_server = Arc::new(GatewayServer::new(port));
            
            if http_only {
                info!("Starting OCLAWS HTTP server on port {}", port);
                let http_port = port;
                
                use oclaws_gateway_core::create_http_server;
                let http_server = create_http_server(http_port, (*gateway).clone(), gateway_server.clone()).await?;
                
                if let Err(e) = http_server.start().await {
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
                info!("Starting OCLAWS gateway on port {} (WebSocket)", port);
                
                let ws_server = gateway_server.clone();
                let _ws_handle = tokio::spawn(async move {
                    if let Err(e) = ws_server.start().await {
                        error!("WebSocket server error: {}", e);
                    }
                });
                
                let http_port = port + 1;
                info!("Starting HTTP server on port {} (HTTP)", http_port);
                
                use oclaws_gateway_core::create_http_server;
                let http_server = create_http_server(http_port, (*gateway).clone(), gateway_server.clone()).await?;
                
                let _http_handle = tokio::spawn(async move {
                    if let Err(e) = http_server.start().await {
                        error!("HTTP server error: {}", e);
                    }
                });
                
                tokio::signal::ctrl_c().await?;
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
        
        Commands::Wizard { path } => {
            use wizard::ConfigWizard;
            use wizard::{success, error};
            
            let config_path = path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ConfigManager::default_config_path().unwrap_or_default());
            
            let mut manager = ConfigManager::new(config_path);
            if manager.load().is_ok()
                && !wizard::prompt_yes_no("Configuration already exists. Overwrite?", false) {
                    info!("Aborted.");
                    return Ok(());
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
        
        Commands::Version => {
            println!("oclaws {}", env!("CARGO_PKG_VERSION"));
            println!("Edition: 2024");
        }
    }
    
    Ok(())
}
