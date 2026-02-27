//! TTS directive parsing — extract [[tts:...]] tags from text.

use crate::types::TtsProvider;

#[derive(Debug, Clone)]
pub struct TtsDirective {
    pub text: Option<String>,
    pub provider: Option<TtsProvider>,
    pub voice: Option<String>,
}

/// Parse TTS directives from text, returning cleaned text and directive.
pub fn parse_tts_directives(text: &str) -> (String, Option<TtsDirective>) {
    let re = regex::Regex::new(r"(?s)\[\[tts(?::(\w+))?\]\](.*?)\[\[/tts\]\]");

    let Ok(re) = re else {
        return (text.to_string(), None);
    };

    let Some(caps) = re.captures(text) else {
        return (text.to_string(), None);
    };

    let provider = caps.get(1).and_then(|m| parse_provider(m.as_str()));
    let tts_text = caps.get(2).map(|m| m.as_str().trim().to_string());

    let cleaned = re.replace_all(text, "").trim().to_string();

    let directive = TtsDirective {
        text: tts_text,
        provider,
        voice: None,
    };

    (cleaned, Some(directive))
}

fn parse_provider(s: &str) -> Option<TtsProvider> {
    match s.to_lowercase().as_str() {
        "openai" => Some(TtsProvider::OpenAI),
        "elevenlabs" | "eleven" => Some(TtsProvider::ElevenLabs),
        "edge" => Some(TtsProvider::Edge),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_directive() {
        let (cleaned, dir) = parse_tts_directives("Hello world");
        assert_eq!(cleaned, "Hello world");
        assert!(dir.is_none());
    }

    #[test]
    fn test_basic_directive() {
        let input = "Before [[tts]]speak this[[/tts]] after";
        let (cleaned, dir) = parse_tts_directives(input);
        assert_eq!(cleaned, "Before  after");
        let d = dir.unwrap();
        assert_eq!(d.text.unwrap(), "speak this");
        assert!(d.provider.is_none());
    }

    #[test]
    fn test_provider_directive() {
        let input = "[[tts:openai]]say hello[[/tts]]";
        let (_, dir) = parse_tts_directives(input);
        let d = dir.unwrap();
        assert_eq!(d.provider, Some(TtsProvider::OpenAI));
    }
}
