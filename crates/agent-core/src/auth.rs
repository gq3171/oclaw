use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentials {
    pub provider: String,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub metadata: HashMap<String, String>,
}

impl ProviderCredentials {
    pub fn new(provider: &str) -> Self {
        Self {
            provider: provider.to_string(),
            api_key: None,
            api_secret: None,
            access_token: None,
            refresh_token: None,
            expires_at: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            chrono::Utc::now().timestamp() >= expires_at
        } else {
            false
        }
    }

    pub fn is_valid(&self) -> bool {
        self.api_key.is_some() || self.access_token.is_some()
    }
}

#[async_trait::async_trait]
pub trait AuthProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    
    async fn authenticate(&self, credentials: &ProviderCredentials) -> Result<String, String>;
    
    async fn refresh(&self, credentials: &ProviderCredentials) -> Result<ProviderCredentials, String>;
    
    async fn validate(&self, credentials: &ProviderCredentials) -> bool;
}

pub struct AuthManager {
    credentials: Arc<RwLock<HashMap<String, ProviderCredentials>>>,
    providers: Arc<RwLock<HashMap<String, Box<dyn AuthProvider>>>>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            credentials: Arc::new(RwLock::new(HashMap::new())),
            providers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_credentials(&self, credentials: ProviderCredentials) {
        self.credentials
            .write()
            .await
            .insert(credentials.provider.clone(), credentials);
    }

    pub async fn get_credentials(&self, provider: &str) -> Option<ProviderCredentials> {
        self.credentials.read().await.get(provider).cloned()
    }

    pub async fn remove_credentials(&self, provider: &str) {
        self.credentials.write().await.remove(provider);
    }

    pub async fn list_providers(&self) -> Vec<String> {
        self.credentials.read().await.keys().cloned().collect()
    }

    pub async fn has_valid_credentials(&self, provider: &str) -> bool {
        self.credentials
            .read()
            .await
            .get(provider)
            .map(|c| c.is_valid() && !c.is_expired())
            .unwrap_or(false)
    }

    pub async fn register_provider(&self, provider: Box<dyn AuthProvider>) {
        self.providers
            .write()
            .await
            .insert(provider.provider_name().to_string(), provider);
    }

    pub async fn authenticate(&self, provider: &str) -> Result<String, String> {
        let creds = self.credentials.read().await.get(provider).cloned()
            .ok_or_else(|| format!("No credentials for provider: {}", provider))?;

        if !creds.is_valid() {
            return Err("Invalid credentials".to_string());
        }

        if creds.is_expired() {
            if let Some(provider_impl) = self.providers.read().await.get(provider) {
                let new_creds = provider_impl.refresh(&creds).await?;
                self.credentials.write().await.insert(provider.to_string(), new_creds);
            }
        }

        Ok(creds.api_key.or(creds.access_token).unwrap_or_default())
    }

    pub async fn validate_all(&self) -> HashMap<String, bool> {
        let creds = self.credentials.read().await;
        let mut results = HashMap::new();

        for (provider, cred) in creds.iter() {
            results.insert(provider.clone(), cred.is_valid() && !cred.is_expired());
        }

        results
    }

    pub async fn clear_expired(&self) {
        let mut creds = self.credentials.write().await;
        creds.retain(|_, c| !c.is_expired() || c.api_key.is_some());
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MultiProviderAuth {
    default_provider: String,
    fallback_providers: Vec<String>,
}

impl MultiProviderAuth {
    pub fn new(default: &str) -> Self {
        Self {
            default_provider: default.to_string(),
            fallback_providers: Vec::new(),
        }
    }

    pub fn add_fallback(mut self, provider: &str) -> Self {
        self.fallback_providers.push(provider.to_string());
        self
    }

    pub async fn get_valid_token(&self, auth: &AuthManager) -> Option<String> {
        if auth.has_valid_credentials(&self.default_provider).await {
            if let Ok(token) = auth.authenticate(&self.default_provider).await {
                return Some(token);
            }
        }

        for provider in &self.fallback_providers {
            if auth.has_valid_credentials(provider).await {
                if let Ok(token) = auth.authenticate(provider).await {
                    return Some(token);
                }
            }
        }

        None
    }
}
