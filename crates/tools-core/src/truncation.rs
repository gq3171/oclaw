/// Configuration for tool result truncation.
pub struct TruncationConfig {
    pub max_chars: usize,
    pub max_lines: usize,
    pub truncation_message: String,
}

impl Default for TruncationConfig {
    fn default() -> Self {
        Self {
            max_chars: 50_000,
            max_lines: 500,
            truncation_message: "[truncated]".to_string(),
        }
    }
}

/// Truncate tool output respecting line boundaries.
pub fn truncate_tool_result(result: &str, config: &TruncationConfig) -> String {
    if result.len() <= config.max_chars && result.lines().count() <= config.max_lines {
        return result.to_string();
    }
    smart_truncate(result, config.max_chars.min(config.max_lines * 200))
}

/// Truncate at the nearest line boundary.
pub fn smart_truncate(result: &str, max_chars: usize) -> String {
    if result.len() <= max_chars {
        return result.to_string();
    }
    let slice = &result[..max_chars];
    let cut = slice.rfind('\n').unwrap_or(max_chars);
    let kept = &result[..cut];
    format!(
        "{}\n\n[truncated, showing first {} of {} chars]",
        kept,
        cut,
        result.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_truncation_needed() {
        let s = "hello\nworld";
        assert_eq!(truncate_tool_result(s, &TruncationConfig::default()), s);
    }

    #[test]
    fn truncates_at_line_boundary() {
        let s = "line1\nline2\nline3\nline4";
        let out = smart_truncate(s, 12);
        assert!(out.starts_with("line1\nline2"));
        assert!(out.contains("[truncated"));
    }

    #[test]
    fn handles_no_newlines() {
        let s = "a".repeat(100);
        let out = smart_truncate(&s, 50);
        assert!(out.len() < 120);
        assert!(out.contains("[truncated"));
    }
}
