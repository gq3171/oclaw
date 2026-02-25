//! Reply payload normalization — sanitize before delivery.

use crate::tokens;
use crate::types::ReplyPayload;

/// Options for normalizing a reply payload.
#[derive(Debug, Clone, Default)]
pub struct NormalizeOptions {
    pub strip_silent: bool,
    pub strip_heartbeat: bool,
    pub skip_empty: bool,
}

impl NormalizeOptions {
    pub fn default_outbound() -> Self {
        Self {
            strip_silent: true,
            strip_heartbeat: true,
            skip_empty: true,
        }
    }
}

/// Normalize a reply payload. Returns None if the payload should be dropped.
pub fn normalize_reply_payload(
    payload: ReplyPayload,
    opts: &NormalizeOptions,
) -> Option<ReplyPayload> {
    let mut text = payload.text.clone();

    if let Some(ref t) = text {
        // Strip silent token — drop entire payload
        if opts.strip_silent && tokens::is_silent(t) {
            return None;
        }
        // Strip heartbeat tokens
        if opts.strip_heartbeat {
            text = tokens::strip_heartbeat(t);
        }
    }

    let has_media = payload.media_url.is_some()
        || payload.media_urls.as_ref().is_some_and(|u| !u.is_empty())
        || payload.channel_data.is_some();

    // Skip empty payloads (no text and no media)
    if opts.skip_empty && text.as_ref().is_none_or(|t| t.is_empty()) && !has_media {
        return None;
    }

    Some(ReplyPayload { text, ..payload })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_silent() {
        let p = ReplyPayload {
            text: Some("[[SILENT]]".into()),
            ..Default::default()
        };
        let opts = NormalizeOptions::default_outbound();
        assert!(normalize_reply_payload(p, &opts).is_none());
    }

    #[test]
    fn strips_heartbeat() {
        let p = ReplyPayload {
            text: Some("hello [[HEARTBEAT]] world".into()),
            ..Default::default()
        };
        let opts = NormalizeOptions::default_outbound();
        let result = normalize_reply_payload(p, &opts).unwrap();
        assert_eq!(result.text.unwrap(), "hello  world");
    }

    #[test]
    fn skips_empty() {
        let p = ReplyPayload {
            text: Some("".into()),
            ..Default::default()
        };
        let opts = NormalizeOptions::default_outbound();
        assert!(normalize_reply_payload(p, &opts).is_none());
    }

    #[test]
    fn keeps_media_without_text() {
        let p = ReplyPayload {
            text: None,
            media_url: Some("https://example.com/img.png".into()),
            ..Default::default()
        };
        let opts = NormalizeOptions::default_outbound();
        assert!(normalize_reply_payload(p, &opts).is_some());
    }
}
