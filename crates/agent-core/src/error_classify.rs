//! Error classification for agent retry/fallback decisions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// 429 or rate-limit message — retry with backoff
    RateLimit,
    /// 401/403 or auth failure — do not retry, switch key or abort
    AuthFailure,
    /// Context window exceeded — compact/truncate then retry
    ContextOverflow,
    /// DNS, timeout, connection reset — retry with backoff
    NetworkError,
    /// 5xx or provider-side failure — retry or fallback model
    ProviderError,
    /// Tool execution failure — surface to LLM, do not retry LLM call
    ToolError,
    /// Unrecognised — default retry policy
    Unknown,
}

/// Classify an error string (typically from LlmError or anyhow) into an ErrorClass.
pub fn classify_error(err: &str) -> ErrorClass {
    let lower = err.to_lowercase();

    // Rate limiting (including AWS Bedrock ThrottlingException)
    if lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("429")
        || lower.contains("too many requests")
        || lower.contains("quota exceeded")
        || lower.contains("throttlingexception")
        || lower.contains("throttling")
    {
        return ErrorClass::RateLimit;
    }

    // Auth failures
    if lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("invalid api key")
        || lower.contains("invalid_api_key")
        || lower.contains("authentication")
    {
        return ErrorClass::AuthFailure;
    }

    // Context overflow
    if lower.contains("context length exceeded")
        || lower.contains("maximum context")
        || lower.contains("too many tokens")
        || lower.contains("content_too_large")
        || lower.contains("request too large")
        || lower.contains("context_length")
    {
        return ErrorClass::ContextOverflow;
    }

    // Network errors
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("dns")
        || lower.contains("network")
        || lower.contains("eof")
    {
        return ErrorClass::NetworkError;
    }

    // Provider / server errors
    if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("internal server error")
        || lower.contains("service unavailable")
        || lower.contains("bad gateway")
        || lower.contains("overloaded")
    {
        return ErrorClass::ProviderError;
    }

    // Tool errors
    if lower.contains("tool error") || lower.contains("tool execution") || lower.contains("tool '")
    {
        return ErrorClass::ToolError;
    }

    ErrorClass::Unknown
}

impl ErrorClass {
    /// Whether this error class should trigger a retry.
    pub fn should_retry(&self) -> bool {
        matches!(
            self,
            Self::RateLimit | Self::NetworkError | Self::ProviderError | Self::Unknown
        )
    }

    /// Whether this error class should trigger model fallback.
    pub fn should_fallback(&self) -> bool {
        matches!(self, Self::ProviderError | Self::AuthFailure)
    }

    /// Whether this error class is fatal (no retry, no fallback).
    /// AuthFailure is NOT fatal — it should attempt fallback to another provider/key.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::ToolError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit() {
        assert_eq!(
            classify_error("HTTP 429: Too Many Requests"),
            ErrorClass::RateLimit
        );
        assert_eq!(classify_error("rate limit exceeded"), ErrorClass::RateLimit);
    }

    #[test]
    fn test_auth() {
        assert_eq!(
            classify_error("HTTP 401 Unauthorized"),
            ErrorClass::AuthFailure
        );
        assert_eq!(
            classify_error("Invalid API key provided"),
            ErrorClass::AuthFailure
        );
    }

    #[test]
    fn test_context_overflow() {
        assert_eq!(
            classify_error("context length exceeded"),
            ErrorClass::ContextOverflow
        );
        assert_eq!(
            classify_error("content_too_large"),
            ErrorClass::ContextOverflow
        );
    }

    #[test]
    fn test_network() {
        assert_eq!(
            classify_error("connection timed out"),
            ErrorClass::NetworkError
        );
        assert_eq!(
            classify_error("DNS resolution failed"),
            ErrorClass::NetworkError
        );
    }

    #[test]
    fn test_provider() {
        assert_eq!(
            classify_error("HTTP 503 Service Unavailable"),
            ErrorClass::ProviderError
        );
        assert_eq!(
            classify_error("server overloaded"),
            ErrorClass::ProviderError
        );
    }

    #[test]
    fn test_unknown() {
        assert_eq!(
            classify_error("something weird happened"),
            ErrorClass::Unknown
        );
    }
}
