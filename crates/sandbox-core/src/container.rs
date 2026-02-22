use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerConfig {
    pub image: String,
    pub tag: String,
    pub command: Vec<String>,
    pub entrypoint: Option<Vec<String>>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub hostname: Option<String>,
    pub domainname: Option<String>,
    pub mac_address: Option<String>,
    pub network_disabled: bool,
    pub interactive: bool,
    pub tty: bool,
    pub auto_remove: bool,
}

impl ContainerConfig {
    pub fn new(image: &str) -> Self {
        Self {
            image: image.to_string(),
            tag: "latest".to_string(),
            command: vec![],
            entrypoint: None,
            env: HashMap::new(),
            working_dir: None,
            user: None,
            hostname: None,
            domainname: None,
            mac_address: None,
            network_disabled: false,
            interactive: false,
            tty: false,
            auto_remove: false,
        }
    }

    pub fn with_command(mut self, cmd: Vec<&str>) -> Self {
        self.command = cmd.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_working_dir(mut self, dir: &str) -> Self {
        self.working_dir = Some(dir.to_string());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerStatus {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLimits {
    pub cpu_shares: Option<i64>,
    pub cpu_quota: Option<i64>,
    pub cpu_period: Option<i64>,
    pub memory_limit: Option<i64>,
    pub memory_swap: Option<i64>,
    pub memory_reservation: Option<i64>,
    pub pids_limit: Option<i64>,
    pub oom_score_adj: Option<i64>,
    pub ulimits: Option<HashMap<String, Ulimit>>,
    pub blkio_weight: Option<i64>,
    pub devices: Option<Vec<DeviceMapping>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ulimit {
    pub name: String,
    pub soft: u64,
    pub hard: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceMapping {
    pub path_on_host: String,
    pub path_in_container: String,
    pub cgroup_permissions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub config: ContainerConfig,
    pub status: ContainerStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub image_id: Option<String>,
    pub host_config: HostConfig,
    pub networks: Vec<String>,
    pub mounts: Vec<MountPoint>,
    pub logs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostConfig {
    pub network_mode: String,
    pub privileged: bool,
    pub read_only: bool,
    pub tmpfs: Option<HashMap<String, String>>,
    pub sysctls: Option<HashMap<String, String>>,
    pub cap_add: Vec<String>,
    pub cap_drop: Vec<String>,
    pub security_opt: Vec<String>,
    pub ulimits: Vec<Ulimit>,
    pub resources: Option<ResourceLimits>,
    pub port_bindings: Option<HashMap<String, Vec<PortBinding>>>,
    pub binds: Vec<String>,
    pub volumes_from: Vec<String>,
    pub dns: Vec<String>,
    pub dns_options: Vec<String>,
    pub dns_search: Vec<String>,
    pub restart_policy: Option<RestartPolicy>,
    pub blocked_paths: Vec<String>,
    pub allowed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortBinding {
    pub host_ip: String,
    pub host_port: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartPolicy {
    pub name: String,
    pub maximum_retry_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountPoint {
    pub source: String,
    pub destination: String,
    pub mode: String,
    pub rw: bool,
    pub propagation: String,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self::new("alpine")
    }
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            network_mode: "bridge".to_string(),
            privileged: false,
            read_only: false,
            tmpfs: None,
            sysctls: None,
            cap_add: Vec::new(),
            cap_drop: vec!["ALL".to_string()],
            security_opt: Vec::new(),
            ulimits: Vec::new(),
            resources: None,
            port_bindings: None,
            binds: Vec::new(),
            volumes_from: Vec::new(),
            dns: Vec::new(),
            dns_options: Vec::new(),
            dns_search: Vec::new(),
            restart_policy: None,
            blocked_paths: vec![
                "/etc".to_string(),
                "/proc".to_string(),
                "/sys".to_string(),
                "/dev".to_string(),
                "/root".to_string(),
                "/boot".to_string(),
            ],
            allowed_paths: Vec::new(),
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu_shares: Some(1024),
            cpu_quota: None,
            cpu_period: Some(100000),
            memory_limit: Some(512 * 1024 * 1024),
            memory_swap: None,
            memory_reservation: Some(256 * 1024 * 1024),
            pids_limit: Some(100),
            oom_score_adj: None,
            ulimits: None,
            blkio_weight: None,
            devices: None,
        }
    }
}

pub struct ContainerManager {
    containers: std::collections::HashMap<String, Container>,
    default_host_config: HostConfig,
}

impl ContainerManager {
    pub fn new() -> Self {
        Self {
            containers: std::collections::HashMap::new(),
            default_host_config: HostConfig::default(),
        }
    }

    pub fn create(&mut self, name: &str, config: ContainerConfig) -> Container {
        let container = Container {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            config,
            status: ContainerStatus::Created,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            exit_code: None,
            image_id: None,
            host_config: self.default_host_config.clone(),
            networks: Vec::new(),
            mounts: Vec::new(),
            logs: None,
        };

        self.containers
            .insert(container.id.clone(), container.clone());
        container
    }

    pub fn get(&self, id: &str) -> Option<&Container> {
        self.containers.get(id)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&Container> {
        self.containers.values().find(|c| c.name == name)
    }

    pub fn list(&self) -> Vec<&Container> {
        self.containers.values().collect()
    }

    pub fn remove(&mut self, id: &str) -> Option<Container> {
        self.containers.remove(id)
    }

    pub fn update_status(&mut self, id: &str, status: ContainerStatus) {
        if let Some(container) = self.containers.get_mut(id) {
            container.status = status;
        }
    }

    pub fn validate_security(
        &self,
        config: &ContainerConfig,
        host_config: &HostConfig,
    ) -> Result<(), String> {
        for blocked in &host_config.blocked_paths {
            if config
                .working_dir
                .as_ref()
                .map(|d| d.starts_with(blocked))
                .unwrap_or(false)
            {
                return Err(format!(
                    "Working directory cannot be in blocked path: {}",
                    blocked
                ));
            }

            for env_key in config.env.keys() {
                if env_key.starts_with(blocked) || env_key.contains(blocked) {
                    return Err(format!(
                        "Environment variable cannot reference blocked path: {}",
                        blocked
                    ));
                }
            }
        }

        for allowed in &host_config.allowed_paths {
            let blocked_count = host_config
                .blocked_paths
                .iter()
                .filter(|b| b.starts_with(allowed))
                .count();
            if blocked_count > 0 {
                return Err(format!(
                    "Allowed path {} conflicts with {} blocked paths",
                    allowed, blocked_count
                ));
            }
        }

        if host_config.privileged {
            return Err("Privileged containers are not allowed".to_string());
        }

        Ok(())
    }
}

impl Default for ContainerManager {
    fn default() -> Self {
        Self::new()
    }
}
