use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub internal: bool,
    pub attachable: bool,
    pub ingress: bool,
    pub created: DateTime<Utc>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub ipam_driver: String,
    pub options: std::collections::HashMap<String, String>,
    pub labels: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpamConfig {
    pub subnet: Option<String>,
    pub ip_range: Option<String>,
    pub gateway: Option<String>,
}

pub struct NetworkManager {
    default_driver: String,
    default_ipam_driver: String,
}

impl NetworkManager {
    pub fn new() -> Self {
        Self {
            default_driver: "bridge".to_string(),
            default_ipam_driver: "default".to_string(),
        }
    }

    pub fn with_driver(mut self, driver: &str) -> Self {
        self.default_driver = driver.to_string();
        self
    }

    pub fn with_ipam_driver(mut self, driver: &str) -> Self {
        self.default_ipam_driver = driver.to_string();
        self
    }

    pub fn create_bridge_config(&self, subnet: &str, gateway: &str) -> Network {
        Network {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!(
                "bridge_{}",
                &uuid::Uuid::new_v4().to_string()[..8]
            ),
            driver: self.default_driver.clone(),
            scope: "local".to_string(),
            internal: false,
            attachable: true,
            ingress: false,
            created: Utc::now(),
            subnet: Some(subnet.to_string()),
            gateway: Some(gateway.to_string()),
            ipam_driver: self.default_ipam_driver.clone(),
            options: std::collections::HashMap::new(),
            labels: std::collections::HashMap::new(),
        }
    }
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}
