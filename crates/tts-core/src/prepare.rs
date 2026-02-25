//! Text preprocessing for TTS — clean up text before synthesis.

/// Prepare text for TTS synthesis.
pub fn prepare_for_tts(text: &str) -> String {
    let text = strip_markdown(text);
    let text = strip_code_blocks(&text);
    let text = collapse_whitespace(&text);
    text.trim().to_string()
}

fn strip_markdown(text: &str) -> String {
    // Remove bold/italic markers
    let text = text.replace("**", "").replace("__", "");
    let text = text.replace(['*', '_'], "");
    // Remove headers
    let re = regex::Regex::new(r"(?m)^#{1,6}\s+").unwrap();
    re.replace_all(&text, "").to_string()
}

fn strip_code_blocks(text: &str) -> String {
    let re = regex::Regex::new(r"(?s)```[^\n]*\n.*?```").unwrap();
    let text = re.replace_all(text, "[code block omitted]");
    let re_inline = regex::Regex::new(r"`[^`]+`").unwrap();
    re_inline.replace_all(&text, "").to_string()
}

fn collapse_whitespace(text: &str) -> String {
    let re = regex::Regex::new(r"\n{3,}").unwrap();
    re.replace_all(text, "\n\n").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_markdown() {
        assert_eq!(prepare_for_tts("**bold** text"), "bold text");
    }

    #[test]
    fn test_strip_code() {
        let input = "Before ```rust\nfn main() {}\n``` after";
        let result = prepare_for_tts(input);
        assert!(result.contains("[code block omitted]"));
        assert!(result.contains("after"));
    }
}
