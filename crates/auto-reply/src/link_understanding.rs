//! Link understanding pipeline — extract URLs, fetch, summarize.

use reqwest::Client;
use tracing::{debug, warn};

use crate::link_detect::extract_urls;
use crate::link_runner::fetch_link_content;

#[derive(Debug, Clone)]
pub struct LinkUnderstandingConfig {
    pub enabled: bool,
    pub max_links: usize,
    pub timeout_secs: u64,
    pub max_content_chars: usize,
}

impl Default for LinkUnderstandingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_links: 3,
            timeout_secs: 30,
            max_content_chars: 4000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinkResult {
    pub url: String,
    pub title: Option<String>,
    pub content: String,
}

/// Process text to extract and understand linked content.
pub async fn apply_link_understanding(
    text: &str,
    config: &LinkUnderstandingConfig,
) -> Vec<LinkResult> {
    if !config.enabled {
        return Vec::new();
    }

    let urls = extract_urls(text);
    if urls.is_empty() {
        return Vec::new();
    }

    debug!(count = urls.len(), "Extracted URLs for understanding");

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default();

    let mut results = Vec::new();

    for url in urls.iter().take(config.max_links) {
        match fetch_link_content(&client, url, config.timeout_secs).await {
            Ok(fetched) => {
                let content = truncate_content(&fetched.text, config.max_content_chars);
                results.push(LinkResult {
                    url: fetched.url,
                    title: fetched.title,
                    content,
                });
            }
            Err(e) => {
                warn!(url = %url, error = %e, "Failed to fetch link");
            }
        }
    }

    results
}

fn truncate_content(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{}…", truncated)
}

/// Format link results as context text for the agent.
pub fn format_link_context(results: &[LinkResult]) -> Option<String> {
    if results.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    for r in results {
        let title = r.title.as_deref().unwrap_or("(no title)");
        parts.push(format!(
            "[Link: {} — {}]\n{}",
            title, r.url, r.content
        ));
    }

    Some(parts.join("\n\n"))
}
