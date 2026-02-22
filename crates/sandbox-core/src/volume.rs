use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created_at: DateTime<Utc>,
    pub labels: std::collections::HashMap<String, String>,
    pub scope: String,
    pub options: std::collections::HashMap<String, String>,
}

pub struct VolumeManager {
    default_driver: String,
}

impl VolumeManager {
    pub fn new() -> Self {
        Self {
            default_driver: "local".to_string(),
        }
    }

    pub fn with_driver(mut self, driver: &str) -> Self {
        self.default_driver = driver.to_string();
        self
    }

    pub fn default_driver(&self) -> &str {
        &self.default_driver
    }
}

impl Default for VolumeManager {
    fn default() -> Self {
        Self::new()
    }
}
