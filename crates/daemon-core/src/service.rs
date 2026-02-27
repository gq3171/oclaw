//! Daemon Service Management - Real implementation

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::{DaemonError, DaemonResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ServiceState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Restarting,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<PathBuf>,
    pub auto_restart: bool,
    pub max_restarts: u32,
}

impl ServiceConfig {
    pub fn new(name: &str, program: PathBuf) -> Self {
        Self {
            name: name.to_string(),
            program,
            args: vec![],
            env: HashMap::new(),
            working_dir: None,
            auto_restart: true,
            max_restarts: 5,
        }
    }
}

pub struct DaemonService {
    config: ServiceConfig,
    state: ServiceState,
    pid: Option<u32>,
    restart_count: u32,
}

impl DaemonService {
    pub fn new(config: ServiceConfig) -> Self {
        Self {
            config,
            state: ServiceState::Stopped,
            pid: None,
            restart_count: 0,
        }
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }
    pub fn state(&self) -> ServiceState {
        self.state
    }
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    async fn spawn(&mut self) -> DaemonResult<()> {
        self.state = ServiceState::Starting;
        let mut cmd = Command::new(&self.config.program);
        cmd.args(&self.config.args);
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &self.config.working_dir {
            cmd.current_dir(dir);
        }
        cmd.kill_on_drop(false);

        match cmd.spawn() {
            Ok(child) => {
                self.pid = child.id();
                self.state = ServiceState::Running;
                info!(name = %self.config.name, pid = ?self.pid, "service started");
                Ok(())
            }
            Err(e) => {
                self.state = ServiceState::Failed;
                error!(name = %self.config.name, err = %e, "service failed to start");
                Err(DaemonError::ServiceError(e.to_string()))
            }
        }
    }

    fn stop_process(&mut self) {
        if let Some(pid) = self.pid.take() {
            self.state = ServiceState::Stopping;
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
            }
            self.state = ServiceState::Stopped;
            info!(name = %self.config.name, pid, "service stopped");
        }
    }
}

#[derive(Clone)]
pub struct ServiceManager {
    services: Arc<RwLock<HashMap<String, DaemonService>>>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, service: DaemonService) -> DaemonResult<()> {
        let name = service.config.name.clone();
        self.services.write().await.insert(name, service);
        Ok(())
    }

    pub async fn start(&self, name: &str) -> DaemonResult<()> {
        let mut services = self.services.write().await;
        let svc = services
            .get_mut(name)
            .ok_or_else(|| DaemonError::NotFound(name.into()))?;
        svc.spawn().await
    }

    pub async fn stop(&self, name: &str) -> DaemonResult<()> {
        let mut services = self.services.write().await;
        let svc = services
            .get_mut(name)
            .ok_or_else(|| DaemonError::NotFound(name.into()))?;
        svc.stop_process();
        Ok(())
    }

    pub async fn restart(&self, name: &str) -> DaemonResult<()> {
        {
            let mut services = self.services.write().await;
            if let Some(svc) = services.get_mut(name) {
                svc.stop_process();
                svc.restart_count += 1;
                svc.state = ServiceState::Restarting;
            }
        }
        self.start(name).await
    }

    pub async fn status(&self, name: &str) -> DaemonResult<(ServiceState, Option<u32>)> {
        let services = self.services.read().await;
        let svc = services
            .get(name)
            .ok_or_else(|| DaemonError::NotFound(name.into()))?;
        Ok((svc.state, svc.pid))
    }

    pub async fn list(&self) -> Vec<(String, ServiceState, Option<u32>)> {
        self.services
            .read()
            .await
            .iter()
            .map(|(n, s)| (n.clone(), s.state, s.pid))
            .collect()
    }
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}
