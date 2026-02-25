//! Special tokens used in the auto-reply pipeline.

/// When an agent returns this token, the reply is suppressed (not sent to channel).
pub const SILENT_REPLY_TOKEN: &str = "[[SILENT]]";

/// Heartbeat token — stripped before delivery, used for keep-alive signaling.
pub const HEARTBEAT_TOKEN: &str = "[[HEARTBEAT]]";

/// Check if text contains only a silent token.
pub fn is_silent(text: &str) -> bool {
    text.trim() == SILENT_REPLY_TOKEN
}

/// Check if text is a heartbeat-only message.
pub fn is_heartbeat(text: &str) -> bool {
    text.trim() == HEARTBEAT_TOKEN
}

/// Strip all heartbeat tokens from text, returning None if nothing remains.
pub fn strip_heartbeat(text: &str) -> Option<String> {
    let cleaned = text.replace(HEARTBEAT_TOKEN, "");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_detection() {
        assert!(is_silent("[[SILENT]]"));
        assert!(is_silent("  [[SILENT]]  "));
        assert!(!is_silent("hello [[SILENT]]"));
    }

    #[test]
    fn heartbeat_strip() {
        assert_eq!(strip_heartbeat("[[HEARTBEAT]]"), None);
        assert_eq!(
            strip_heartbeat("hello [[HEARTBEAT]] world"),
            Some("hello  world".to_string())
        );
    }
}
