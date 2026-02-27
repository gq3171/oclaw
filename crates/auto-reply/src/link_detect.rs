//! URL extraction and SSRF protection.

use std::net::IpAddr;

/// Extract URLs from text, filtering out markdown image links.
pub fn extract_urls(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"https?://[^\s\)<>\]\}"'`]+"#).unwrap();

    re.find_iter(text)
        .map(|m| {
            m.as_str()
                .trim_end_matches(['.', ',', ';', ':', '!', '?'])
                .to_string()
        })
        .filter(|u| url::Url::parse(u).is_ok())
        .collect()
}

/// Strip markdown-style links, returning only the display text.
pub fn strip_markdown_links(text: &str) -> String {
    let re = regex::Regex::new(r"\[([^\]]*)\]\([^\)]+\)").unwrap();
    re.replace_all(text, "$1").to_string()
}

/// Check if a host is blocked for SSRF protection.
pub fn is_blocked_host(host: &str) -> bool {
    // Block private/reserved IP ranges
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(&ip);
    }

    let lower = host.to_lowercase();
    let blocked_hosts = [
        "localhost",
        "127.0.0.1",
        "0.0.0.0",
        "[::1]",
        "metadata.google.internal",
        "169.254.169.254",
    ];

    if blocked_hosts.iter().any(|b| lower == *b) {
        return true;
    }

    // Block .local and .internal TLDs
    if lower.ends_with(".local") || lower.ends_with(".internal") {
        return true;
    }

    false
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.octets()[0] == 169 && v4.octets()[1] == 254
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_urls() {
        let text = "Check https://example.com and http://foo.bar/path?q=1 out";
        let urls = extract_urls(text);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com");
        assert_eq!(urls[1], "http://foo.bar/path?q=1");
    }

    #[test]
    fn test_extract_urls_trailing_punct() {
        let text = "Visit https://example.com.";
        let urls = extract_urls(text);
        assert_eq!(urls[0], "https://example.com");
    }

    #[test]
    fn test_strip_markdown_links() {
        let text = "See [Google](https://google.com) for more.";
        assert_eq!(strip_markdown_links(text), "See Google for more.");
    }

    #[test]
    fn test_blocked_hosts() {
        assert!(is_blocked_host("localhost"));
        assert!(is_blocked_host("127.0.0.1"));
        assert!(is_blocked_host("169.254.169.254"));
        assert!(is_blocked_host("foo.local"));
        assert!(!is_blocked_host("example.com"));
    }
}
