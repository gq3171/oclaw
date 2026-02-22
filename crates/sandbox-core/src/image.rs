use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub created: DateTime<Utc>,
    pub size: i64,
    pub virtual_size: Option<i64>,
    pub labels: std::collections::HashMap<String, String>,
    pub architecture: String,
    pub os: String,
    pub config: Option<ImageConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    pub hostname: String,
    pub domainname: String,
    pub user: String,
    pub attach_stdin: bool,
    pub attach_stdout: bool,
    pub attach_stderr: bool,
    pub tty: bool,
    pub open_stdin: bool,
    pub stdin_once: bool,
    pub env: Vec<String>,
    pub cmd: Vec<String>,
    pub image: String,
    pub volumes: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub working_dir: String,
    pub entrypoint: Option<Vec<String>>,
    pub labels: std::collections::HashMap<String, String>,
}

pub struct ImageManager {
    registry_auth: std::collections::HashMap<String, RegistryAuth>,
}

#[derive(Debug, Clone)]
pub struct RegistryAuth {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub server_address: String,
}

impl ImageManager {
    pub fn new() -> Self {
        Self {
            registry_auth: std::collections::HashMap::new(),
        }
    }

    pub fn add_registry_auth(&mut self, server: &str, auth: RegistryAuth) {
        self.registry_auth.insert(server.to_string(), auth);
    }

    pub fn get_auth(&self, server: &str) -> Option<&RegistryAuth> {
        self.registry_auth.get(server)
    }
}

impl Default for ImageManager {
    fn default() -> Self {
        Self::new()
    }
}
