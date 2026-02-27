//! Multi-key rotation and auth profile management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileType {
    ApiKey,
    Token {
        expires_at: Option<i64>,
    },
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProfile {
    pub id: String,
    pub provider: String,
    pub profile_type: ProfileType,
    pub credentials: String,
    #[serde(default)]
    pub cooldown_until: Option<i64>,
    #[serde(default)]
    pub disabled_until: Option<i64>,
    #[serde(default)]
    pub error_count: u32,
    #[serde(default)]
    pub last_used: Option<i64>,
    #[serde(default)]
    pub last_good: Option<i64>,
}

impl AuthProfile {
    fn is_available(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        if let Some(cd) = self.cooldown_until
            && now < cd
        {
            return false;
        }
        if let Some(dis) = self.disabled_until
            && now < dis
        {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthProfileStoreData {
    pub profiles: HashMap<String, AuthProfile>,
    #[serde(default)]
    pub order: HashMap<String, Vec<String>>,
}

pub struct AuthProfileStore {
    data: AuthProfileStoreData,
    path: PathBuf,
}

impl AuthProfileStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            data: AuthProfileStoreData::default(),
            path,
        }
    }

    pub fn load(path: PathBuf) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new(path));
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read auth profiles: {}", e))?;
        let data: AuthProfileStoreData = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse auth profiles: {}", e))?;
        Ok(Self { data, path })
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
        }
        let tmp = self.path.with_extension("tmp");
        let json = serde_json::to_string_pretty(&self.data)
            .map_err(|e| format!("Failed to serialize: {}", e))?;
        std::fs::write(&tmp, &json).map_err(|e| format!("Failed to write: {}", e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("Failed to rename: {}", e))?;
        Ok(())
    }

    pub fn resolve_profile(&self, provider: &str) -> Option<&AuthProfile> {
        // Try ordered list first
        if let Some(order) = self.data.order.get(provider) {
            for id in order {
                if let Some(p) = self.data.profiles.get(id)
                    && p.is_available()
                {
                    return Some(p);
                }
            }
        }
        // Fallback: pick by last_good time
        self.data
            .profiles
            .values()
            .filter(|p| p.provider == provider && p.is_available())
            .max_by_key(|p| p.last_good.unwrap_or(0))
    }

    pub fn report_error(&mut self, profile_id: &str) {
        if let Some(p) = self.data.profiles.get_mut(profile_id) {
            p.error_count += 1;
            let now = chrono::Utc::now().timestamp();
            // Exponential cooldown: 30s * 2^(errors-1), max 1 hour
            let cooldown = (30 * 2i64.pow(p.error_count.saturating_sub(1).min(6))).min(3600);
            p.cooldown_until = Some(now + cooldown);
            let _ = self.save();
        }
    }

    pub fn report_success(&mut self, profile_id: &str) {
        if let Some(p) = self.data.profiles.get_mut(profile_id) {
            p.error_count = 0;
            p.cooldown_until = None;
            p.disabled_until = None;
            let now = chrono::Utc::now().timestamp();
            p.last_used = Some(now);
            p.last_good = Some(now);
            let _ = self.save();
        }
    }

    pub fn add_profile(&mut self, profile: AuthProfile) {
        self.data.profiles.insert(profile.id.clone(), profile);
        let _ = self.save();
    }

    pub fn remove_profile(&mut self, id: &str) {
        self.data.profiles.remove(id);
        let _ = self.save();
    }

    pub fn list_profiles(&self) -> Vec<&AuthProfile> {
        self.data.profiles.values().collect()
    }
}
