//! Device pairing authentication — 6-digit code exchange for long-term device tokens.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

const CODE_EXPIRY_SECS: i64 = 300; // 5 minutes

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCode {
    pub code: String,
    pub device_name: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub device_id: String,
    pub device_name: String,
    pub token: String,
    pub paired_at: i64,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedDevices {
    devices: Vec<PairedDevice>,
}

pub struct DevicePairingManager {
    pending: Arc<RwLock<HashMap<String, PendingCode>>>,
    devices: Arc<RwLock<Vec<PairedDevice>>>,
    token_index: Arc<RwLock<HashMap<String, usize>>>,
    persist_path: Option<PathBuf>,
}

impl DevicePairingManager {
    pub fn new(persist_path: Option<PathBuf>) -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            devices: Arc::new(RwLock::new(Vec::new())),
            token_index: Arc::new(RwLock::new(HashMap::new())),
            persist_path,
        }
    }

    /// Load paired devices from disk.
    pub async fn load(&self) -> Result<(), String> {
        let Some(path) = &self.persist_path else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read device store: {}", e))?;
        let data: PersistedDevices = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse device store: {}", e))?;

        let mut devices = self.devices.write().await;
        let mut index = self.token_index.write().await;
        *devices = data.devices;
        index.clear();
        for (i, d) in devices.iter().enumerate() {
            index.insert(d.token.clone(), i);
        }
        Ok(())
    }

    /// Persist paired devices to disk.
    async fn save(&self) -> Result<(), String> {
        let Some(path) = &self.persist_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
        }
        let devices = self.devices.read().await;
        let data = PersistedDevices {
            devices: devices.clone(),
        };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize: {}", e))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &json).map_err(|e| format!("Failed to write: {}", e))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("Failed to rename: {}", e))?;
        Ok(())
    }

    /// Generate a 6-digit pairing code. Returns the code string.
    pub async fn generate_code(&self, device_name: Option<&str>) -> String {
        let code = Self::random_code();
        let now = chrono::Utc::now().timestamp();
        let pending = PendingCode {
            code: code.clone(),
            device_name: device_name.map(String::from),
            created_at: now,
            expires_at: now + CODE_EXPIRY_SECS,
        };
        let mut store = self.pending.write().await;
        store.insert(code.clone(), pending);
        code
    }

    /// Verify a pairing code and issue a long-term device token.
    pub async fn verify_code(&self, code: &str) -> Result<PairedDevice, DevicePairingError> {
        let mut store = self.pending.write().await;
        let pending = store.remove(code).ok_or(DevicePairingError::InvalidCode)?;

        let now = chrono::Utc::now().timestamp();
        if now > pending.expires_at {
            return Err(DevicePairingError::CodeExpired);
        }

        let device = PairedDevice {
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: pending
                .device_name
                .unwrap_or_else(|| "Unknown Device".into()),
            token: Self::generate_token(),
            paired_at: now,
            last_seen: Some(now),
        };

        drop(store);

        let mut devices = self.devices.write().await;
        let idx = devices.len();
        devices.push(device.clone());
        let mut index = self.token_index.write().await;
        index.insert(device.token.clone(), idx);
        drop(devices);
        drop(index);

        let _ = self.save().await;
        Ok(device)
    }

    /// Validate a device token. Returns the device_id if valid.
    pub async fn validate_token(&self, token: &str) -> Option<String> {
        let index = self.token_index.read().await;
        let idx = index.get(token)?;
        let devices = self.devices.read().await;
        devices.get(*idx).map(|d| d.device_id.clone())
    }

    /// Update last_seen timestamp for a device token.
    pub async fn touch(&self, token: &str) {
        let index = self.token_index.read().await;
        if let Some(&idx) = index.get(token) {
            drop(index);
            let mut devices = self.devices.write().await;
            if let Some(d) = devices.get_mut(idx) {
                d.last_seen = Some(chrono::Utc::now().timestamp());
            }
        }
    }

    /// List all paired devices.
    pub async fn list_devices(&self) -> Vec<PairedDevice> {
        self.devices.read().await.clone()
    }

    /// Revoke a paired device by device_id.
    pub async fn revoke_device(&self, device_id: &str) -> Result<(), DevicePairingError> {
        let mut devices = self.devices.write().await;
        let pos = devices
            .iter()
            .position(|d| d.device_id == device_id)
            .ok_or(DevicePairingError::DeviceNotFound)?;
        let removed = devices.remove(pos);

        let mut index = self.token_index.write().await;
        index.remove(&removed.token);
        // Rebuild index after removal
        index.clear();
        for (i, d) in devices.iter().enumerate() {
            index.insert(d.token.clone(), i);
        }
        drop(devices);
        drop(index);

        let _ = self.save().await;
        Ok(())
    }

    /// Clean up expired pending codes.
    pub async fn cleanup_expired_codes(&self) {
        let now = chrono::Utc::now().timestamp();
        let mut store = self.pending.write().await;
        store.retain(|_, p| p.expires_at > now);
    }

    fn random_code() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let n: u32 = rng.random_range(0..1_000_000);
        format!("{:06}", n)
    }

    fn generate_token() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
        format!(
            "dev_{}",
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
        )
    }
}

#[derive(Debug)]
pub enum DevicePairingError {
    InvalidCode,
    CodeExpired,
    DeviceNotFound,
}

impl std::fmt::Display for DevicePairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCode => write!(f, "Invalid pairing code"),
            Self::CodeExpired => write!(f, "Pairing code expired"),
            Self::DeviceNotFound => write!(f, "Device not found"),
        }
    }
}

impl std::error::Error for DevicePairingError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generate_and_verify_code() {
        let mgr = DevicePairingManager::new(None);
        let code = mgr.generate_code(Some("Test Phone")).await;
        assert_eq!(code.len(), 6);

        let device = mgr.verify_code(&code).await.unwrap();
        assert_eq!(device.device_name, "Test Phone");
        assert!(device.token.starts_with("dev_"));
    }

    #[tokio::test]
    async fn test_invalid_code() {
        let mgr = DevicePairingManager::new(None);
        let result = mgr.verify_code("000000").await;
        assert!(matches!(result, Err(DevicePairingError::InvalidCode)));
    }

    #[tokio::test]
    async fn test_code_single_use() {
        let mgr = DevicePairingManager::new(None);
        let code = mgr.generate_code(None).await;
        mgr.verify_code(&code).await.unwrap();
        // Second use should fail
        let result = mgr.verify_code(&code).await;
        assert!(matches!(result, Err(DevicePairingError::InvalidCode)));
    }

    #[tokio::test]
    async fn test_validate_device_token() {
        let mgr = DevicePairingManager::new(None);
        let code = mgr.generate_code(None).await;
        let device = mgr.verify_code(&code).await.unwrap();

        let id = mgr.validate_token(&device.token).await;
        assert_eq!(id, Some(device.device_id));
    }

    #[tokio::test]
    async fn test_revoke_device() {
        let mgr = DevicePairingManager::new(None);
        let code = mgr.generate_code(None).await;
        let device = mgr.verify_code(&code).await.unwrap();

        mgr.revoke_device(&device.device_id).await.unwrap();
        assert!(mgr.validate_token(&device.token).await.is_none());
        assert!(mgr.list_devices().await.is_empty());
    }
}
