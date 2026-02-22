use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    pub host: String,
    pub api_version: String,
    pub timeout_secs: u64,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            host: "unix:///var/run/docker.sock".to_string(),
            api_version: "1.43".to_string(),
            timeout_secs: 30,
        }
    }
}

pub struct DockerClient {
    config: DockerConfig,
    client: reqwest::Client,
}

impl DockerClient {
    pub fn new(config: DockerConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_default();

        Self { config, client }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}{}", self.config.host, self.config.api_version, path)
    }

    pub async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        let url = self.api_url(&format!("/containers/json?all={}", all));
        let response = self.client.get(&url).send().await?;
        let containers: Vec<ContainerInfo> = response.json().await?;
        Ok(containers)
    }

    pub async fn create_container(&self, config: serde_json::Value) -> Result<CreateResponse> {
        let url = self.api_url("/containers/create");
        let response = self.client.post(&url).json(&config).send().await?;
        let create_resp: CreateResponse = response.json().await?;
        Ok(create_resp)
    }

    pub async fn start_container(&self, id: &str) -> Result<()> {
        let url = self.api_url(&format!("/containers/{}/start", id));
        self.client.post(&url).send().await?;
        Ok(())
    }

    pub async fn stop_container(&self, id: &str, t: Option<i32>) -> Result<()> {
        let url = self.api_url(&format!(
            "/containers/{}/stop?t={}",
            id,
            t.unwrap_or(10)
        ));
        self.client.post(&url).send().await?;
        Ok(())
    }

    pub async fn remove_container(&self, id: &str, force: bool, v: bool) -> Result<()> {
        let url = self.api_url(&format!(
            "/containers/{}?force={}&v={}",
            id, force, v
        ));
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn inspect_container(&self, id: &str) -> Result<serde_json::Value> {
        let url = self.api_url(&format!("/containers/{}/json", id));
        let response = self.client.get(&url).send().await?;
        let info: serde_json::Value = response.json().await?;
        Ok(info)
    }

    pub async fn logs(&self, id: &str, stdout: bool, stderr: bool, tail: usize) -> Result<String> {
        let url = self.api_url(&format!(
            "/containers/{}/logs?stdout={}&stderr={}&tail={}",
            id, stdout as i32, stderr as i32, tail
        ));
        let response = self.client.get(&url).send().await?;
        let bytes = response.bytes().await?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    pub async fn exec_create(&self, container_id: &str, cmd: Vec<&str>, tty: bool) -> Result<String> {
        let url = self.api_url(&format!("/containers/{}/exec", container_id));
        let config = serde_json::json!({
            "AttachStdout": true,
            "AttachStderr": true,
            "Tty": tty,
            "Cmd": cmd
        });
        let response = self.client.post(&url).json(&config).send().await?;
        let resp: CreateResponse = response.json().await?;
        Ok(resp.id)
    }

    pub async fn exec_start(&self, exec_id: &str, tty: bool) -> Result<String> {
        let url = self.api_url(&format!("/exec/{}/start", exec_id));
        let config = serde_json::json!({
            "Detach": false,
            "Tty": tty
        });
        let response = self.client.post(&url).json(&config).send().await?;
        let bytes = response.bytes().await?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    pub async fn list_images(&self) -> Result<Vec<ImageInfo>> {
        let url = self.api_url("/images/json");
        let response = self.client.get(&url).send().await?;
        let images: Vec<ImageInfo> = response.json().await?;
        Ok(images)
    }

    pub async fn pull_image(&self, image: &str, tag: &str) -> Result<()> {
        let url = self.api_url(&format!("/images/create?fromImage={}&tag={}", image, tag));
        self.client.post(&url).send().await?;
        Ok(())
    }

    pub async fn remove_image(&self, id: &str, force: bool) -> Result<()> {
        let url = self.api_url(&format!("/images/{}?force={}", id, force));
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn list_volumes(&self) -> Result<Vec<VolumeInfo>> {
        let url = self.api_url("/volumes");
        let response = self.client.get(&url).send().await?;
        #[derive(Deserialize)]
        struct VolumesResponse {
            Volumes: Vec<VolumeInfo>,
        }
        let volumes: VolumesResponse = response.json().await?;
        Ok(volumes.Volumes)
    }

    pub async fn create_volume(&self, name: &str, driver: &str) -> Result<VolumeInfo> {
        let url = self.api_url("/volumes/create");
        let config = serde_json::json!({
            "Name": name,
            "Driver": driver
        });
        let response = self.client.post(&url).json(&config).send().await?;
        let volume: VolumeInfo = response.json().await?;
        Ok(volume)
    }

    pub async fn remove_volume(&self, name: &str, force: bool) -> Result<()> {
        let url = self.api_url(&format!("/volumes/{}?force={}", name, force));
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn list_networks(&self) -> Result<Vec<NetworkInfo>> {
        let url = self.api_url("/networks");
        let response = self.client.get(&url).send().await?;
        let networks: Vec<NetworkInfo> = response.json().await?;
        Ok(networks)
    }

    pub async fn create_network(&self, name: &str, driver: &str) -> Result<NetworkInfo> {
        let url = self.api_url("/networks/create");
        let config = serde_json::json!({
            "Name": name,
            "Driver": driver
        });
        let response = self.client.post(&url).json(&config).send().await?;
        let network: NetworkInfo = response.json().await?;
        Ok(network)
    }

    pub async fn remove_network(&self, id: &str) -> Result<()> {
        let url = self.api_url(&format!("/networks/{}", id));
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn system_info(&self) -> Result<serde_json::Value> {
        let url = self.api_url("/info");
        let response = self.client.get(&url).send().await?;
        let info: serde_json::Value = response.json().await?;
        Ok(info)
    }

    pub async fn version(&self) -> Result<VersionInfo> {
        let url = self.api_url("/version");
        let response = self.client.get(&url).send().await?;
        let version: VersionInfo = response.json().await?;
        Ok(version)
    }

    pub async fn ping(&self) -> Result<bool> {
        let url = self.api_url("/_ping");
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub names: Vec<String>,
    pub image: String,
    pub image_id: String,
    pub command: String,
    pub created: i64,
    pub state: String,
    pub status: String,
    pub ports: Vec<Port>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Port {
    #[serde(rename = "IP")]
    pub ip: Option<String>,
    pub private_port: u16,
    pub public_port: Option<u16>,
    #[serde(rename = "Type")]
    pub type_: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageInfo {
    pub id: String,
    pub repo_tags: Option<Vec<String>>,
    pub repo_digests: Option<Vec<String>>,
    pub created: i64,
    pub size: i64,
    pub virtual_size: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VolumeInfo {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created_at: String,
    pub scope: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub created: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateResponse {
    pub id: String,
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    pub version: String,
    #[serde(rename = "ApiVersion")]
    pub api_version: String,
    pub os: String,
    pub arch: String,
    #[serde(rename = "KernelVersion")]
    pub kernel_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub container_id: String,
    pub cpu_stats: CpuStats,
    pub memory_stats: MemoryStats,
    pub network_stats: HashMap<String, NetworkStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuStats {
    pub cpu_usage: CpuUsage,
    #[serde(rename = "system_cpu_usage")]
    pub system_cpu_usage: u64,
    #[serde(rename = "online_cpus")]
    pub online_cpus: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuUsage {
    #[serde(rename = "total_usage")]
    pub total_usage: u64,
    #[serde(rename = "percpu_usage")]
    pub percpu_usage: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub usage: u64,
    pub limit: u64,
    pub stats: MemoryDetailStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDetailStats {
    pub cache: u64,
    pub rss: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    #[serde(rename = "rx_bytes")]
    pub rx_bytes: u64,
    #[serde(rename = "tx_bytes")]
    pub tx_bytes: u64,
    #[serde(rename = "rx_packets")]
    pub rx_packets: u64,
    #[serde(rename = "tx_packets")]
    pub tx_packets: u64,
}

pub struct DockerSandbox {
    client: DockerClient,
    containers: Arc<RwLock<HashMap<String, ContainerState>>>,
}

#[derive(Debug, Clone)]
pub struct ContainerState {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DockerSandbox {
    pub fn new(config: DockerConfig) -> Self {
        Self {
            client: DockerClient::new(config),
            containers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn health_check(&self) -> Result<bool> {
        self.client.ping().await
    }

    pub async fn get_containers(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        self.client.list_containers(all).await
    }

    pub async fn create_sandbox(&self, name: &str, image: &str) -> Result<String> {
        let config = serde_json::json!({
            "Image": image,
            "name": name,
            "Env": [
                "SANDBOX=true"
            ],
            "Cmd": ["sleep", "infinity"],
            "HostConfig": {
                "NetworkMode": "none",
                "Memory": 512 * 1024 * 1024,
                "CpuPeriod": 100000,
                "CpuQuota": 50000,
                "PidsLimit": 100
            }
        });

        let response = self.client.create_container(config).await?;
        
        let mut containers = self.containers.write().await;
        containers.insert(response.id.clone(), ContainerState {
            id: response.id.clone(),
            name: name.to_string(),
            status: "created".to_string(),
            created_at: chrono::Utc::now(),
        });

        Ok(response.id)
    }

    pub async fn start_sandbox(&self, id: &str) -> Result<()> {
        self.client.start_container(id).await?;
        
        let mut containers = self.containers.write().await;
        if let Some(state) = containers.get_mut(id) {
            state.status = "running".to_string();
        }
        
        Ok(())
    }

    pub async fn stop_sandbox(&self, id: &str) -> Result<()> {
        self.client.stop_container(id, Some(10)).await?;
        
        let mut containers = self.containers.write().await;
        if let Some(state) = containers.get_mut(id) {
            state.status = "stopped".to_string();
        }
        
        Ok(())
    }

    pub async fn remove_sandbox(&self, id: &str) -> Result<()> {
        self.client.remove_container(id, true, false).await?;
        
        let mut containers = self.containers.write().await;
        containers.remove(id);
        
        Ok(())
    }

    pub async fn exec_in_sandbox(&self, id: &str, cmd: Vec<&str>) -> Result<String> {
        let exec_id = self.client.exec_create(id, cmd.clone(), false).await?;
        self.client.exec_start(&exec_id, false).await
    }

    pub async fn get_sandbox_logs(&self, id: &str, tail: usize) -> Result<String> {
        self.client.logs(id, true, true, tail).await
    }

    pub async fn list_sandboxes(&self) -> Vec<ContainerState> {
        let containers = self.containers.read().await;
        containers.values().cloned().collect()
    }

    pub async fn get_sandbox(&self, id: &str) -> Option<ContainerState> {
        let containers = self.containers.read().await;
        containers.get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_config_default() {
        let config = DockerConfig::default();
        assert_eq!(config.host, "unix:///var/run/docker.sock");
        assert_eq!(config.api_version, "1.43");
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn test_docker_config_custom() {
        let config = DockerConfig {
            host: "tcp://localhost:2375".to_string(),
            api_version: "1.44".to_string(),
            timeout_secs: 60,
        };
        assert_eq!(config.host, "tcp://localhost:2375");
        assert_eq!(config.api_version, "1.44");
        assert_eq!(config.timeout_secs, 60);
    }

    #[test]
    fn test_container_state() {
        let state = ContainerState {
            id: "test-id".to_string(),
            name: "test-container".to_string(),
            status: "running".to_string(),
            created_at: chrono::Utc::now(),
        };
        assert_eq!(state.status, "running");
    }
}
