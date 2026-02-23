use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OAuthProvider {
    Google,
    Discord,
    GitHub,
    Slack,
    Custom,
}

impl OAuthProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            OAuthProvider::Google => "google",
            OAuthProvider::Discord => "discord",
            OAuthProvider::GitHub => "github",
            OAuthProvider::Slack => "slack",
            OAuthProvider::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "google" => Some(OAuthProvider::Google),
            "discord" => Some(OAuthProvider::Discord),
            "github" => Some(OAuthProvider::GitHub),
            "slack" => Some(OAuthProvider::Slack),
            "custom" => Some(OAuthProvider::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub provider: OAuthProvider,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub auth_url: String,
    pub token_url: String,
    pub user_info_url: String,
}

impl OAuthConfig {
    pub fn google(client_id: &str, client_secret: &str, redirect_uri: &str) -> Self {
        Self {
            provider: OAuthProvider::Google,
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri: redirect_uri.to_string(),
            scopes: vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ],
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            user_info_url: "https://www.googleapis.com/oauth2/v2/userinfo".to_string(),
        }
    }

    pub fn discord(client_id: &str, client_secret: &str, redirect_uri: &str) -> Self {
        Self {
            provider: OAuthProvider::Discord,
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri: redirect_uri.to_string(),
            scopes: vec!["identify".to_string(), "email".to_string()],
            auth_url: "https://discord.com/api/oauth2/authorize".to_string(),
            token_url: "https://discord.com/api/oauth2/token".to_string(),
            user_info_url: "https://discord.com/api/users/@me".to_string(),
        }
    }

    pub fn github(client_id: &str, client_secret: &str, redirect_uri: &str) -> Self {
        Self {
            provider: OAuthProvider::GitHub,
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri: redirect_uri.to_string(),
            scopes: vec!["user:email".to_string(), "read:user".to_string()],
            auth_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            user_info_url: "https://api.github.com/user".to_string(),
        }
    }

    pub fn get_auth_url(&self, state: &str) -> String {
        let scopes = self.scopes.join(" ");
        let client_id = urlencoding::encode(&self.client_id);
        let redirect_uri = urlencoding::encode(&self.redirect_uri);
        let state_encoded = urlencoding::encode(state);
        
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
            self.auth_url, client_id, redirect_uri, scopes, state_encoded
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: Option<String>,
    pub created_at: i64,
}

impl OAuthToken {
    pub fn is_expired(&self) -> bool {
        let expiry = self.created_at + self.expires_in as i64;
        chrono::Utc::now().timestamp() >= expiry
    }

    pub fn refresh_needed(&self) -> bool {
        let expiry = self.created_at + (self.expires_in as i64 - 300);
        chrono::Utc::now().timestamp() >= expiry
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

pub struct OAuthStateStore {
    states: Mutex<HashMap<String, Instant>>,
    ttl: Duration,
    max_entries: usize,
}

impl OAuthStateStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
            ttl,
            max_entries: 10_000,
        }
    }

    pub fn generate_state(&self) -> String {
        use rand::Rng;
        let bytes: [u8; 16] = rand::rng().random();
        let state = hex::encode(bytes);
        let mut states = self.states.lock().unwrap();
        if states.len() >= self.max_entries {
            let ttl = self.ttl;
            states.retain(|_, created| created.elapsed() < ttl);
        }
        states.insert(state.clone(), Instant::now());
        state
    }

    pub fn validate_state(&self, state: &str) -> bool {
        let mut states = self.states.lock().unwrap();
        match states.remove(state) {
            Some(created) => created.elapsed() < self.ttl,
            None => false,
        }
    }

    pub fn cleanup_expired(&self) {
        let mut states = self.states.lock().unwrap();
        let ttl = self.ttl;
        states.retain(|_, created| created.elapsed() < ttl);
    }
}

pub struct OAuthClient {
    config: OAuthConfig,
    http_client: reqwest::Client,
}

impl OAuthClient {
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn get_auth_url(&self, state: &str) -> String {
        self.config.get_auth_url(state)
    }

    pub async fn exchange_code(&self, code: &str) -> Result<OAuthToken, OAuthError> {
        let params = [
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", self.config.redirect_uri.as_str()),
        ];

        let response = self
            .http_client
            .post(&self.config.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| OAuthError::Network(e.to_string()))?;

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: Option<String>,
            token_type: String,
            expires_in: Option<u64>,
            scope: Option<String>,
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| OAuthError::Parse(e.to_string()))?;

        Ok(OAuthToken {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            token_type: token_response.token_type,
            expires_in: token_response.expires_in.unwrap_or(3600),
            scope: token_response.scope,
            created_at: chrono::Utc::now().timestamp(),
        })
    }

    pub async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthToken, OAuthError> {
        let params = [
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .http_client
            .post(&self.config.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| OAuthError::Network(e.to_string()))?;

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: Option<String>,
            token_type: String,
            expires_in: Option<u64>,
            scope: Option<String>,
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| OAuthError::Parse(e.to_string()))?;

        Ok(OAuthToken {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token.or(Some(refresh_token.to_string())),
            token_type: token_response.token_type,
            expires_in: token_response.expires_in.unwrap_or(3600),
            scope: token_response.scope,
            created_at: chrono::Utc::now().timestamp(),
        })
    }

    pub async fn get_user_info(&self, token: &OAuthToken) -> Result<OAuthUser, OAuthError> {
        let response = self
            .http_client
            .get(&self.config.user_info_url)
            .header("Authorization", format!("Bearer {}", token.access_token))
            .send()
            .await
            .map_err(|e| OAuthError::Network(e.to_string()))?;

        match self.config.provider {
            OAuthProvider::Google => {
                #[derive(Deserialize)]
                struct GoogleUser {
                    id: String,
                    name: String,
                    email: Option<String>,
                    picture: Option<String>,
                }

                let user: GoogleUser = response
                    .json()
                    .await
                    .map_err(|e| OAuthError::Parse(e.to_string()))?;

                Ok(OAuthUser {
                    id: user.id,
                    username: user.name.clone(),
                    display_name: user.name,
                    email: user.email,
                    avatar_url: user.picture,
                })
            }
            OAuthProvider::Discord => {
                #[derive(Deserialize)]
                struct DiscordUser {
                    id: String,
                    username: String,
                    global_name: Option<String>,
                    email: Option<String>,
                    avatar: Option<String>,
                }

                let user: DiscordUser = response
                    .json()
                    .await
                    .map_err(|e| OAuthError::Parse(e.to_string()))?;

                let avatar_url = user.avatar.map(|av| {
                    format!("https://cdn.discordapp.com/avatars/{}/{}.png", user.id, av)
                });

                Ok(OAuthUser {
                    id: user.id,
                    username: user.username.clone(),
                    display_name: user.global_name.unwrap_or(user.username),
                    email: user.email,
                    avatar_url,
                })
            }
            OAuthProvider::GitHub => {
                #[derive(Deserialize)]
                struct GitHubUser {
                    id: u64,
                    login: String,
                    name: Option<String>,
                    email: Option<String>,
                    avatar_url: Option<String>,
                }

                let user: GitHubUser = response
                    .json()
                    .await
                    .map_err(|e| OAuthError::Parse(e.to_string()))?;

                Ok(OAuthUser {
                    id: user.id.to_string(),
                    username: user.login.clone(),
                    display_name: user.name.unwrap_or(user.login),
                    email: user.email,
                    avatar_url: user.avatar_url,
                })
            }
            _ => Err(OAuthError::UnsupportedProvider),
        }
    }
}

#[derive(Debug)]
pub enum OAuthError {
    Network(String),
    Parse(String),
    Token(String),
    UnsupportedProvider,
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthError::Network(msg) => write!(f, "Network error: {}", msg),
            OAuthError::Parse(msg) => write!(f, "Parse error: {}", msg),
            OAuthError::Token(msg) => write!(f, "Token error: {}", msg),
            OAuthError::UnsupportedProvider => write!(f, "Unsupported OAuth provider"),
        }
    }
}

impl std::error::Error for OAuthError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_parse_round_trip() {
        for name in ["google", "discord", "github", "slack", "custom"] {
            let p = OAuthProvider::parse(name).unwrap();
            assert_eq!(p.as_str(), name);
        }
        assert!(OAuthProvider::parse("unknown").is_none());
    }

    #[test]
    fn test_auth_url_generation() {
        let cfg = OAuthConfig::google("cid", "csec", "http://localhost/cb");
        let url = cfg.get_auth_url("state123");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("redirect_uri=http"));
        assert!(url.contains("state=state123"));
    }

    #[test]
    fn test_discord_config_urls() {
        let cfg = OAuthConfig::discord("id", "sec", "http://cb");
        assert_eq!(cfg.auth_url, "https://discord.com/api/oauth2/authorize");
        assert_eq!(cfg.token_url, "https://discord.com/api/oauth2/token");
    }

    #[test]
    fn test_github_config_urls() {
        let cfg = OAuthConfig::github("id", "sec", "http://cb");
        assert_eq!(cfg.auth_url, "https://github.com/login/oauth/authorize");
    }

    #[test]
    fn test_token_expiry() {
        let token = OAuthToken {
            access_token: "at".into(),
            refresh_token: None,
            token_type: "Bearer".into(),
            expires_in: 3600,
            scope: None,
            created_at: chrono::Utc::now().timestamp() - 7200,
        };
        assert!(token.is_expired());
        assert!(token.refresh_needed());
    }

    #[test]
    fn test_token_not_expired() {
        let token = OAuthToken {
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            token_type: "Bearer".into(),
            expires_in: 3600,
            scope: Some("openid".into()),
            created_at: chrono::Utc::now().timestamp(),
        };
        assert!(!token.is_expired());
        assert!(!token.refresh_needed());
    }

    #[test]
    fn test_token_refresh_needed_within_buffer() {
        // Created 3400s ago with 3600s expiry → 200s left, under 300s buffer
        let token = OAuthToken {
            access_token: "at".into(),
            refresh_token: None,
            token_type: "Bearer".into(),
            expires_in: 3600,
            scope: None,
            created_at: chrono::Utc::now().timestamp() - 3400,
        };
        assert!(!token.is_expired());
        assert!(token.refresh_needed());
    }

    #[test]
    fn test_state_store_generate_and_validate() {
        let store = OAuthStateStore::new(Duration::from_secs(300));
        let state = store.generate_state();
        assert_eq!(state.len(), 32); // 16 bytes = 32 hex chars
        assert!(store.validate_state(&state));
    }

    #[test]
    fn test_state_store_reuse_fails() {
        let store = OAuthStateStore::new(Duration::from_secs(300));
        let state = store.generate_state();
        assert!(store.validate_state(&state));
        assert!(!store.validate_state(&state)); // second use fails
    }

    #[test]
    fn test_state_store_unknown_fails() {
        let store = OAuthStateStore::new(Duration::from_secs(300));
        assert!(!store.validate_state("bogus"));
    }

    #[test]
    fn test_state_store_expired_fails() {
        let store = OAuthStateStore::new(Duration::from_millis(0));
        let state = store.generate_state();
        std::thread::sleep(Duration::from_millis(1));
        assert!(!store.validate_state(&state));
    }

    #[test]
    fn test_state_store_cleanup() {
        let store = OAuthStateStore::new(Duration::from_millis(0));
        store.generate_state();
        store.generate_state();
        std::thread::sleep(Duration::from_millis(1));
        store.cleanup_expired();
        assert!(store.states.lock().unwrap().is_empty());
    }
}
