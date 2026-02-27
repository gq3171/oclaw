/// Group activation gating: decide whether to process a message based on
/// activation mode (mention vs always) and mention detection.
use regex::Regex;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupActivation {
    #[default]
    Mention,
    Always,
}

pub fn normalize_activation(raw: Option<&str>) -> Option<GroupActivation> {
    match raw?.trim().to_lowercase().as_str() {
        "mention" => Some(GroupActivation::Mention),
        "always" => Some(GroupActivation::Always),
        _ => None,
    }
}

/// Build mention-detection regexes from agent identity.
pub fn build_mention_patterns(name: Option<&str>, extra: &[String]) -> Vec<Regex> {
    let mut patterns = Vec::new();
    if let Some(n) = name {
        let escaped = regex::escape(n.trim());
        if !escaped.is_empty()
            && let Ok(re) = Regex::new(&format!(r"(?i)\b@?{}\b", escaped))
        {
            patterns.push(re);
        }
    }
    for p in extra {
        if let Ok(re) = Regex::new(&format!("(?i){}", p)) {
            patterns.push(re);
        }
    }
    patterns
}

/// Check if text contains a mention matching any pattern.
pub fn is_mentioned(text: &str, patterns: &[Regex]) -> bool {
    patterns.iter().any(|re| re.is_match(text))
}

/// Gating decision: should we process this message?
pub fn should_process(is_group: bool, activation: GroupActivation, mentioned: bool) -> bool {
    if !is_group {
        return true; // DMs always processed
    }
    match activation {
        GroupActivation::Always => true,
        GroupActivation::Mention => mentioned,
    }
}
