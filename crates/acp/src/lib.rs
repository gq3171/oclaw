//! ACP — Agent Communication Protocol.

pub mod permissions;
pub mod server;
pub mod session;
pub mod translator;
pub mod types;

pub use permissions::{AcpPermissions, PermissionDecision};
pub use server::AcpServer;
pub use session::{AcpSession, AcpSessionError, AcpSessionStore};
pub use types::{AcpMessage, AcpRole, AcpToolCall, AcpToolResult};

// ─── Rate Limiting ───────────────────────────────────────────────────

/// Maximum prompt request size (2MB, matching Node's MAX_PROMPT_BYTES).
pub const MAX_PROMPT_BYTES: usize = 2 * 1024 * 1024;

/// Maximum session creations per time window.
pub const SESSION_RATE_LIMIT_MAX: u32 = 120;

/// Rate limit time window in milliseconds.
pub const SESSION_RATE_LIMIT_WINDOW_MS: u64 = 10_000;

/// Simple sliding-window rate limiter for session creation.
///
/// Uses `parking_lot::Mutex` so the limiter can be shared across async tasks
/// without requiring `&mut self`.
pub struct SessionRateLimiter {
    max_requests: u32,
    window_ms: u64,
    timestamps: parking_lot::Mutex<std::collections::VecDeque<u64>>,
}

impl SessionRateLimiter {
    pub fn new(max_requests: u32, window_ms: u64) -> Self {
        Self {
            max_requests,
            window_ms,
            timestamps: parking_lot::Mutex::new(std::collections::VecDeque::new()),
        }
    }

    pub fn default_session_limiter() -> Self {
        Self::new(SESSION_RATE_LIMIT_MAX, SESSION_RATE_LIMIT_WINDOW_MS)
    }

    /// Returns `Ok(())` if request is allowed, `Err` if rate limit exceeded.
    pub fn check(&self) -> Result<(), &'static str> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut ts = self.timestamps.lock();

        // Remove entries outside the window
        let cutoff = now.saturating_sub(self.window_ms);
        while ts.front().map(|&t| t < cutoff).unwrap_or(false) {
            ts.pop_front();
        }

        if ts.len() as u32 >= self.max_requests {
            return Err("Session creation rate limit exceeded");
        }

        ts.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod rate_limit_tests {
    use super::*;

    #[test]
    fn allows_within_limit() {
        let limiter = SessionRateLimiter::new(3, 10_000);
        assert!(limiter.check().is_ok());
        assert!(limiter.check().is_ok());
        assert!(limiter.check().is_ok());
    }

    #[test]
    fn blocks_over_limit() {
        let limiter = SessionRateLimiter::new(2, 10_000);
        assert!(limiter.check().is_ok());
        assert!(limiter.check().is_ok());
        assert!(limiter.check().is_err());
    }

    #[test]
    fn default_limiter_constants() {
        let limiter = SessionRateLimiter::default_session_limiter();
        assert_eq!(limiter.max_requests, SESSION_RATE_LIMIT_MAX);
        assert_eq!(limiter.window_ms, SESSION_RATE_LIMIT_WINDOW_MS);
    }
}
