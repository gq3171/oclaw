mod oauth;
mod pairing;
mod session;
pub mod audit;

pub use oauth::{OAuthProvider, OAuthClient, OAuthToken, OAuthUser, OAuthStateStore};
pub use audit::{AuditLog, AuditEvent, AuditEventKind};
pub use pairing::{DMPairing, PairingRequest, PairingManager};
pub use session::{SecuritySession, SessionManager};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub oauth_enabled: bool,
    pub dm_pairing_enabled: bool,
    pub session_timeout_secs: u64,
    pub max_sessions_per_user: usize,
    pub require_approval: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            oauth_enabled: false,
            dm_pairing_enabled: true,
            session_timeout_secs: 3600,
            max_sessions_per_user: 5,
            require_approval: true,
        }
    }
}

pub trait AuthProvider: Send + Sync {
    fn authenticate(&self, credentials: &Credentials) -> Result<AuthUser, AuthError>;
    fn validate_token(&self, token: &str) -> Result<AuthUser, AuthError>;
    fn refresh_token(&self, refresh_token: &str) -> Result<OAuthToken, AuthError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
    pub oauth_provider: Option<String>,
    pub oauth_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub roles: Vec<String>,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthError {
    InvalidCredentials,
    TokenExpired,
    TokenInvalid,
    UserNotFound,
    UserDisabled,
    RateLimited,
    OAuthError(String),
    Internal(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "Invalid credentials"),
            AuthError::TokenExpired => write!(f, "Token expired"),
            AuthError::TokenInvalid => write!(f, "Token invalid"),
            AuthError::UserNotFound => write!(f, "User not found"),
            AuthError::UserDisabled => write!(f, "User disabled"),
            AuthError::RateLimited => write!(f, "Rate limited"),
            AuthError::OAuthError(msg) => write!(f, "OAuth error: {}", msg),
            AuthError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for AuthError {}

pub struct AuthManager {
    providers: HashMap<String, Box<dyn AuthProvider>>,
    sessions: SessionManager,
    #[allow(dead_code)]
    config: SecurityConfig,
}

impl AuthManager {
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            providers: HashMap::new(),
            sessions: SessionManager::new(config.session_timeout_secs),
            config,
        }
    }

    pub fn register_provider(&mut self, name: String, provider: Box<dyn AuthProvider>) {
        self.providers.insert(name, provider);
    }

    pub fn authenticate(&self, credentials: &Credentials) -> Result<AuthUser, AuthError> {
        if let Some(ref token) = credentials.token
            && let Some(provider) = self.providers.get("token")
        {
            return provider.validate_token(token);
        }

        if credentials.oauth_code.is_some()
            && let Some(ref provider_name) = credentials.oauth_provider
            && let Some(provider) = self.providers.get(provider_name)
        {
            return provider.authenticate(credentials);
        }

        Err(AuthError::InvalidCredentials)
    }

    pub async fn create_session(&self, user: &AuthUser) -> Result<String, AuthError> {
        self.sessions.create_session(user).await
    }

    pub async fn validate_session(&self, session_token: &str) -> Result<AuthUser, AuthError> {
        self.sessions.validate_session(session_token).await
    }

    pub async fn revoke_session(&self, session_token: &str) -> Result<(), AuthError> {
        self.sessions.revoke_session(session_token).await
    }

    pub async fn cleanup_expired(&self) {
        self.sessions.cleanup_expired().await;
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new(SecurityConfig::default())
    }
}
