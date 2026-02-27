//! Content fetching and markdown conversion for links.

use reqwest::Client;
use tracing::debug;

use crate::link_detect::is_blocked_host;

/// Fetch a URL and return its content as simplified text.
pub async fn fetch_link_content(
    client: &Client,
    url: &str,
    timeout_secs: u64,
) -> Result<FetchedContent, LinkFetchError> {
    let parsed = url::Url::parse(url).map_err(|e| LinkFetchError::InvalidUrl(e.to_string()))?;

    // SSRF check
    if let Some(host) = parsed.host_str()
        && is_blocked_host(host)
    {
        return Err(LinkFetchError::Blocked(host.to_string()));
    }

    debug!(url = %url, "Fetching link content");

    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .header("User-Agent", "oclaw-bot/0.1")
        .send()
        .await
        .map_err(|e| LinkFetchError::Request(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(LinkFetchError::HttpStatus(status.as_u16()));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Only process text-based content
    if !is_text_content(&content_type) {
        return Err(LinkFetchError::UnsupportedContent(content_type));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| LinkFetchError::Request(e.to_string()))?;

    let title = extract_title(&body);
    let text = html_to_plain(&body);

    Ok(FetchedContent {
        url: url.to_string(),
        title,
        text,
        content_type,
    })
}

#[derive(Debug, Clone)]
pub struct FetchedContent {
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub content_type: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LinkFetchError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("blocked host: {0}")]
    Blocked(String),
    #[error("request failed: {0}")]
    Request(String),
    #[error("HTTP {0}")]
    HttpStatus(u16),
    #[error("unsupported content type: {0}")]
    UnsupportedContent(String),
}

fn is_text_content(ct: &str) -> bool {
    let lower = ct.to_lowercase();
    lower.contains("text/html")
        || lower.contains("text/plain")
        || lower.contains("application/json")
        || lower.contains("text/xml")
        || lower.contains("application/xml")
}

fn extract_title(html: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)<title[^>]*>(.*?)</title>").ok()?;
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| html_entity_decode(m.as_str().trim()))
}

/// Minimal HTML to plain text conversion.
fn html_to_plain(html: &str) -> String {
    // Remove script and style blocks (separate patterns — regex crate has no backrefs)
    let re_script = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let text = re_script.replace_all(html, "");
    let re_style = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let text = re_style.replace_all(&text, "");

    // Replace block elements with newlines
    let re_block = regex::Regex::new(r"(?i)</(p|div|h[1-6]|li|tr|br\s*/?)>").unwrap();
    let text = re_block.replace_all(&text, "\n");

    // Strip remaining tags
    let re_tags = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re_tags.replace_all(&text, "");

    // Decode entities and collapse whitespace
    let text = html_entity_decode(&text);
    collapse_whitespace(&text)
}

fn html_entity_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_newline = false;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_newline {
                result.push('\n');
                prev_newline = true;
            }
        } else {
            result.push_str(trimmed);
            result.push('\n');
            prev_newline = false;
        }
    }
    result.trim().to_string()
}
