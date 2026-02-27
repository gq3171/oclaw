#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedImageDataUrl {
    pub alt_text: String,
    pub mime_type: String,
    pub base64_data: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParsedMarkdownSegment {
    Text(String),
    Image(ParsedImageDataUrl),
}

fn parse_image_data_url(url: &str, alt_text: &str) -> Option<ParsedImageDataUrl> {
    let trimmed = url.trim();
    if !trimmed.to_ascii_lowercase().starts_with("data:") {
        return None;
    }
    let (meta, payload) = trimmed.split_once(',')?;
    if !meta.to_ascii_lowercase().contains(";base64") {
        return None;
    }
    let mime_type = meta
        .strip_prefix("data:")
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_ascii_lowercase())?;
    if !mime_type.starts_with("image/") {
        return None;
    }
    let base64_data = payload.trim();
    if base64_data.is_empty() {
        return None;
    }
    Some(ParsedImageDataUrl {
        alt_text: alt_text.to_string(),
        mime_type,
        base64_data: base64_data.to_string(),
    })
}

pub(crate) fn parse_markdown_data_url_segments(input: &str) -> Vec<ParsedMarkdownSegment> {
    if input.is_empty() {
        return vec![ParsedMarkdownSegment::Text(String::new())];
    }

    let mut segments = Vec::new();
    let mut cursor = 0usize;
    let total_len = input.len();

    while cursor < total_len {
        let remaining = &input[cursor..];
        let Some(start_rel) = remaining.find("![") else {
            break;
        };
        let start = cursor + start_rel;
        let alt_search_start = start + 2;
        let Some(alt_end_rel) = input[alt_search_start..].find("](") else {
            cursor = alt_search_start;
            continue;
        };
        let alt_end = alt_search_start + alt_end_rel;
        let url_start = alt_end + 2;
        let Some(url_end_rel) = input[url_start..].find(')') else {
            cursor = alt_search_start;
            continue;
        };
        let url_end = url_start + url_end_rel;
        let alt_text = &input[alt_search_start..alt_end];
        let url = &input[url_start..url_end];

        if let Some(image) = parse_image_data_url(url, alt_text) {
            if start > cursor {
                segments.push(ParsedMarkdownSegment::Text(
                    input[cursor..start].to_string(),
                ));
            }
            segments.push(ParsedMarkdownSegment::Image(image));
            cursor = url_end + 1;
            continue;
        }
        cursor = alt_search_start;
    }

    if cursor < total_len {
        segments.push(ParsedMarkdownSegment::Text(input[cursor..].to_string()));
    }

    if segments.is_empty() {
        segments.push(ParsedMarkdownSegment::Text(input.to_string()));
    }
    segments
}

pub(crate) fn markdown_contains_data_url_image(input: &str) -> bool {
    parse_markdown_data_url_segments(input)
        .iter()
        .any(|seg| matches!(seg, ParsedMarkdownSegment::Image(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_image() {
        let src = "hello\n\n![dot](data:image/png;base64,AAAA)";
        let parsed = parse_markdown_data_url_segments(src);
        assert_eq!(parsed.len(), 2);
        assert!(matches!(parsed[0], ParsedMarkdownSegment::Text(_)));
        match &parsed[1] {
            ParsedMarkdownSegment::Image(img) => {
                assert_eq!(img.mime_type, "image/png");
                assert_eq!(img.base64_data, "AAAA");
                assert_eq!(img.alt_text, "dot");
            }
            _ => panic!("expected image segment"),
        }
    }

    #[test]
    fn ignore_non_image_data_url() {
        let src = "![f](data:application/pdf;base64,AAAA)";
        assert!(!markdown_contains_data_url_image(src));
    }
}
