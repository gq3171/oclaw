use hmac::{Hmac, Mac};
use oclaws_config::settings::{GatewayAuth, RateLimit};
use rand::Rng;
use sha2::Sha256;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct AuthState {
    config: Option<GatewayAuth>,
    rate_limiter: Arc<RwLock<RateLimiter>>,
    tokens: Arc<RwLock<HashMap<String, TokenInfo>>>,
    device_sessions: Arc<RwLock<HashMap<String, DeviceSession>>>,
}

#[derive(Clone)]
struct TokenInfo {
    created_at: Instant,
    ttl: Duration,
    scopes: Vec<String>,
}

impl TokenInfo {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

#[derive(Clone)]
struct DeviceSession {
    device_id: String,
    _created_at: Instant,
    _public_key: Option<String>,
}

struct RateLimiter {
    config: Option<RateLimit>,
    attempts: HashMap<IpAddr, RateLimitState>,
}

struct RateLimitState {
    attempts: usize,
    first_attempt: Instant,
    locked_until: Option<Instant>,
}

impl AuthState {
    pub fn new(auth: Option<GatewayAuth>) -> Self {
        Self {
            config: auth.clone(),
            rate_limiter: Arc::new(RwLock::new(RateLimiter::new(auth.and_then(|a| a.rate_limit)))),
            tokens: Arc::new(RwLock::new(HashMap::new())),
            device_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn has_auth_config(&self) -> bool {
        self.config.is_some()
    }

    pub async fn should_allow_connection(&self, client_ip: &IpAddr) -> bool {
        let limiter = self.rate_limiter.read().await;
        limiter.is_allowed(client_ip)
    }

    pub async fn authenticate_token(&self, token: &str) -> Option<Vec<String>> {
        let tokens = self.tokens.read().await;
        tokens.get(token).and_then(|t| {
            if t.is_expired() { None } else { Some(t.scopes.clone()) }
        })
    }

    pub async fn validate_password(&self, password: &str) -> bool {
        if let Some(auth) = &self.config
            && let Some(expected) = &auth.password {
                return password == expected;
            }
        false
    }

    pub async fn register_token(&self, token: String, scopes: Vec<String>) {
        self.register_token_with_ttl(token, scopes, Duration::from_secs(86400)).await;
    }

    pub async fn register_token_with_ttl(&self, token: String, scopes: Vec<String>, ttl: Duration) {
        let mut tokens = self.tokens.write().await;
        tokens.insert(token, TokenInfo { created_at: Instant::now(), ttl, scopes });
    }

    pub async fn cleanup_expired_tokens(&self) {
        let mut tokens = self.tokens.write().await;
        tokens.retain(|_, t| !t.is_expired());
    }

    pub async fn create_device_session(&self, device_id: String) -> String {
        let session_token = generate_token();
        let mut sessions = self.device_sessions.write().await;
        sessions.insert(
            session_token.clone(),
            DeviceSession {
                device_id,
                _created_at: Instant::now(),
                _public_key: None,
            },
        );
        session_token
    }

    pub async fn validate_device_session(&self, session_token: &str) -> Option<String> {
        let sessions = self.device_sessions.read().await;
        sessions.get(session_token).map(|s| s.device_id.clone())
    }
}

impl RateLimiter {
    fn new(config: Option<RateLimit>) -> Self {
        Self {
            config,
            attempts: HashMap::new(),
        }
    }

    fn is_allowed(&self, client_ip: &IpAddr) -> bool {
        let config = match &self.config {
            Some(c) => c,
            None => return true,
        };

        let exempt_loopback = config.exempt_loopback.unwrap_or(true);
        if exempt_loopback && client_ip.is_loopback() {
            return true;
        }

        let max_attempts = config.max_attempts.unwrap_or(10) as usize;
        let window_ms = config.window_ms.unwrap_or(60000);
        let _lockout_ms = config.lockout_ms.unwrap_or(300000);

        let state = match self.attempts.get(client_ip) {
            Some(s) => s,
            None => return true,
        };

        if let Some(locked_until) = state.locked_until
            && Instant::now() < locked_until {
                return false;
            }

        let window = Duration::from_millis(window_ms as u64);
        if state.first_attempt + window < Instant::now() {
            return true;
        }

        state.attempts < max_attempts
    }

    fn _record_attempt(&mut self, client_ip: &IpAddr) {
        let config = match &self.config {
            Some(c) => c,
            None => return,
        };

        let max_attempts = config.max_attempts.unwrap_or(10) as usize;
        let window_ms = config.window_ms.unwrap_or(60000);
        let lockout_ms = config.lockout_ms.unwrap_or(300000);

        let state = self.attempts.entry(*client_ip).or_insert(RateLimitState {
            attempts: 0,
            first_attempt: Instant::now(),
            locked_until: None,
        });

        state.attempts += 1;

        if state.attempts >= max_attempts {
            state.locked_until = Some(Instant::now() + Duration::from_millis(lockout_ms as u64));
        }

        let window = Duration::from_millis(window_ms as u64);
        if state.first_attempt + window < Instant::now() {
            state.attempts = 1;
            state.first_attempt = Instant::now();
            state.locked_until = None;
        }
    }
}

pub struct Authenticator {
    auth_state: Arc<RwLock<AuthState>>,
}

impl Authenticator {
    pub fn new(auth_state: Arc<RwLock<AuthState>>) -> Self {
        Self { auth_state }
    }

    pub async fn authenticate(&self, token: Option<&str>, password: Option<&str>) -> Result<AuthResult, String> {
        if let Some(t) = token {
            let state = self.auth_state.read().await;
            if let Some(scopes) = state.authenticate_token(t).await {
                return Ok(AuthResult {
                    authenticated: true,
                    scopes,
                    device_id: None,
                });
            }
        }

        if let Some(p) = password {
            let state = self.auth_state.read().await;
            if state.validate_password(p).await {
                return Ok(AuthResult {
                    authenticated: true,
                    scopes: vec!["full".to_string()],
                    device_id: None,
                });
            }
        }

        Err("Invalid credentials".to_string())
    }
}

pub struct AuthResult {
    pub authenticated: bool,
    pub scopes: Vec<String>,
    pub device_id: Option<String>,
}

fn generate_token() -> String {
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

pub fn compute_hmac(secret: &str, message: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    let result = mac.finalize();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, result.into_bytes())
}

pub fn verify_hmac(secret: &str, message: &str, signature: &str) -> bool {
    compute_hmac(secret, message) == signature
}
