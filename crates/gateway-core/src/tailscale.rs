use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::error::{GatewayError, GatewayResult};
use oclaw_config::settings::Tailscale;

pub struct TailscaleManager {
    config: Tailscale,
    state: Arc<RwLock<TailscaleState>>,
}

struct TailscaleState {
    is_connected: bool,
    ip_address: Option<String>,
    peers: Vec<String>,
}

impl TailscaleManager {
    pub fn new(config: Tailscale) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(TailscaleState {
                is_connected: false,
                ip_address: None,
                peers: Vec::new(),
            })),
        }
    }

    pub async fn connect(&self) -> GatewayResult<()> {
        let mode = self.config.mode.as_deref().unwrap_or("standalone");

        match mode {
            "standalone" => {
                info!("Tailscale in standalone mode, not connecting");
                return Ok(());
            }
            "managed" => {
                info!("Starting managed Tailscale connection");
            }
            _ => {
                return Err(GatewayError::ConfigError(format!(
                    "Unknown Tailscale mode: {}",
                    mode
                )));
            }
        }

        let mut cmd = Command::new("tailscale");
        cmd.args(["up", "--operator=root"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd
            .output()
            .await
            .map_err(|e| GatewayError::ServerError(format!("Failed to start Tailscale: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GatewayError::ServerError(format!(
                "Tailscale connection failed: {}",
                stderr
            )));
        }

        let mut state = self.state.write().await;
        state.is_connected = true;

        info!("Tailscale connected successfully");
        Ok(())
    }

    pub async fn disconnect(&self) -> GatewayResult<()> {
        let output = Command::new("tailscale")
            .args(["down"])
            .output()
            .await
            .map_err(|e| GatewayError::ServerError(format!("Failed to stop Tailscale: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GatewayError::ServerError(format!(
                "Tailscale disconnect failed: {}",
                stderr
            )));
        }

        let mut state = self.state.write().await;
        state.is_connected = false;
        state.ip_address = None;
        state.peers.clear();

        info!("Tailscale disconnected");
        Ok(())
    }

    pub async fn get_ip_address(&self) -> Option<String> {
        let output = Command::new("tailscale")
            .args(["ip", "-4"])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if ip.is_empty() {
            None
        } else {
            let mut state = self.state.write().await;
            state.ip_address = Some(ip.clone());
            Some(ip)
        }
    }

    pub async fn status(&self) -> TailscaleStatus {
        let state = self.state.read().await;
        TailscaleStatus {
            is_connected: state.is_connected,
            ip_address: state.ip_address.clone(),
            peers: state.peers.clone(),
            mode: self.config.mode.clone().unwrap_or_default(),
        }
    }

    pub async fn reset_if_needed(&self) -> GatewayResult<()> {
        if !self.config.reset_on_exit.unwrap_or(false) {
            return Ok(());
        }

        let output = Command::new("tailscale")
            .args(["down"])
            .output()
            .await
            .map_err(|e| GatewayError::ServerError(format!("Failed to reset Tailscale: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("Tailscale reset warning: {}", stderr);
        }

        let mut state = self.state.write().await;
        state.is_connected = false;
        state.ip_address = None;
        state.peers.clear();

        info!("Tailscale state reset");
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TailscaleStatus {
    pub is_connected: bool,
    pub ip_address: Option<String>,
    pub peers: Vec<String>,
    pub mode: String,
}

pub async fn create_tailscale_manager(
    config: Option<Tailscale>,
) -> GatewayResult<Option<TailscaleManager>> {
    match config {
        Some(cfg) => {
            let manager = TailscaleManager::new(cfg);
            Ok(Some(manager))
        }
        None => Ok(None),
    }
}
