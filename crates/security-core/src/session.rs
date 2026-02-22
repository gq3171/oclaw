use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{AuthUser, AuthError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySession {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_activity: i64,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl SecuritySession {
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now().timestamp() >= self.expires_at
    }

    pub fn is_active(&self) -> bool {
        !self.is_expired()
    }

    pub fn update_activity(&mut self) {
        self.last_activity = chrono::Utc::now().timestamp();
    }
}

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, SecuritySession>>>,
    user_sessions: Arc<RwLock<HashMap<String, Vec<String>>>>,
    timeout_secs: u64,
}

impl SessionManager {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            user_sessions: Arc::new(RwLock::new(HashMap::new())),
            timeout_secs,
        }
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    pub async fn create_session(&self, user: &AuthUser) -> Result<String, AuthError> {
        let token = self.generate_token();
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + self.timeout_secs as i64;

        let session = SecuritySession {
            token: token.clone(),
            user_id: user.id.clone(),
            username: user.username.clone(),
            created_at: now,
            expires_at,
            last_activity: now,
            ip_address: None,
            user_agent: None,
            metadata: HashMap::new(),
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(token.clone(), session);

        let mut user_sessions = self.user_sessions.write().await;
        user_sessions
            .entry(user.id.clone())
            .or_insert_with(Vec::new)
            .push(token.clone());

        Ok(token)
    }

    pub async fn validate_session(&self, token: &str) -> Result<AuthUser, AuthError> {
        let sessions = self.sessions.read().await;

        let session = sessions
            .get(token)
            .ok_or(AuthError::TokenInvalid)?;

        if session.is_expired() {
            drop(sessions);
            self.revoke_session(token).await.ok();
            return Err(AuthError::TokenExpired);
        }

        Ok(AuthUser {
            id: session.user_id.clone(),
            username: session.username.clone(),
            display_name: session.username.clone(),
            email: None,
            avatar_url: None,
            roles: Vec::new(),
            provider: "session".to_string(),
        })
    }

    pub async fn refresh_session(&self, token: &str) -> Result<String, AuthError> {
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(token)
            .ok_or(AuthError::TokenInvalid)?;

        if session.is_expired() {
            return Err(AuthError::TokenExpired);
        }

        let now = chrono::Utc::now().timestamp();
        session.expires_at = now + self.timeout_secs as i64;
        session.last_activity = now;

        Ok(token.to_string())
    }

    pub async fn revoke_session(&self, token: &str) -> Result<(), AuthError> {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(token)
        };

        if let Some(session) = session {
            let mut user_sessions = self.user_sessions.write().await;
            if let Some(tokens) = user_sessions.get_mut(&session.user_id) {
                tokens.retain(|t| t != token);
            }
            Ok(())
        } else {
            Err(AuthError::TokenInvalid)
        }
    }

    pub async fn revoke_all_user_sessions(&self, user_id: &str) -> Result<usize, AuthError> {
        let tokens = {
            let mut user_sessions = self.user_sessions.write().await;
            user_sessions.remove(user_id).unwrap_or_default()
        };

        let count = tokens.len();
        
        let mut sessions = self.sessions.write().await;
        for token in &tokens {
            sessions.remove(token);
        }

        Ok(count)
    }

    pub async fn get_session(&self, token: &str) -> Option<SecuritySession> {
        let sessions = self.sessions.read().await;
        sessions.get(token).cloned()
    }

    pub async fn list_user_sessions(&self, user_id: &str) -> Vec<SecuritySession> {
        let user_sessions = self.user_sessions.read().await;
        let sessions = self.sessions.read().await;
        
        user_sessions
            .get(user_id)
            .map(|tokens| {
                tokens
                    .iter()
                    .filter_map(|t| sessions.get(t).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn cleanup_expired(&self) {
        let now = chrono::Utc::now().timestamp();
        
        let expired_tokens: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions
                .iter()
                .filter(|(_, s)| s.expires_at < now)
                .map(|(t, _)| t.clone())
                .collect()
        };

        for token in expired_tokens {
            self.revoke_session(&token).await.ok();
        }
    }

    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    pub async fn active_user_count(&self) -> usize {
        let user_sessions = self.user_sessions.read().await;
        user_sessions.len()
    }

    fn generate_token(&self) -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let manager = SessionManager::new(3600);
        
        let user = AuthUser {
            id: "user1".to_string(),
            username: "testuser".to_string(),
            display_name: "Test User".to_string(),
            email: None,
            avatar_url: None,
            roles: vec![],
            provider: "test".to_string(),
        };
        
        let token = manager.create_session(&user).await.unwrap();
        assert!(!token.is_empty());
    }

    #[tokio::test]
    async fn test_validate_session() {
        let manager = SessionManager::new(3600);
        
        let user = AuthUser {
            id: "user1".to_string(),
            username: "testuser".to_string(),
            display_name: "Test User".to_string(),
            email: None,
            avatar_url: None,
            roles: vec![],
            provider: "test".to_string(),
        };
        
        let token = manager.create_session(&user).await.unwrap();
        let validated = manager.validate_session(&token).await.unwrap();
        
        assert_eq!(validated.id, "user1");
    }

    #[tokio::test]
    async fn test_revoke_session() {
        let manager = SessionManager::new(3600);
        
        let user = AuthUser {
            id: "user1".to_string(),
            username: "testuser".to_string(),
            display_name: "Test User".to_string(),
            email: None,
            avatar_url: None,
            roles: vec![],
            provider: "test".to_string(),
        };
        
        let token = manager.create_session(&user).await.unwrap();
        manager.revoke_session(&token).await.unwrap();
        
        let result = manager.validate_session(&token).await;
        assert!(result.is_err());
    }
}
