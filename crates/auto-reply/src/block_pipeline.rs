//! Block reply pipeline — deduplication, buffering, and flush control.

use crate::types::ReplyPayload;
use std::collections::HashSet;

/// Generates a dedup key for a reply payload.
fn payload_key(p: &ReplyPayload) -> String {
    let text_part = p.text.as_deref().unwrap_or("");
    let media_part = p.media_url.as_deref().unwrap_or("");
    let reply_part = p.reply_to_id.as_deref().unwrap_or("");
    format!("{}|{}|{}", text_part, media_part, reply_part)
}

/// Pipeline that buffers, deduplicates, and flushes block replies.
pub struct BlockReplyPipeline {
    buffer: Vec<ReplyPayload>,
    seen_keys: HashSet<String>,
    timeout_ms: u64,
    aborted: bool,
    did_stream: bool,
}

impl BlockReplyPipeline {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            buffer: Vec::new(),
            seen_keys: HashSet::new(),
            timeout_ms,
            aborted: false,
            did_stream: false,
        }
    }

    /// Enqueue a payload. Duplicates are silently dropped.
    pub fn enqueue(&mut self, payload: ReplyPayload) {
        if self.aborted {
            return;
        }
        let key = payload_key(&payload);
        if self.seen_keys.contains(&key) {
            return;
        }
        self.seen_keys.insert(key);
        self.buffer.push(payload);
    }

    /// Flush buffered payloads. If `force`, flush everything.
    pub fn flush(&mut self, force: bool) -> Vec<ReplyPayload> {
        if self.aborted {
            return vec![];
        }
        if force || !self.buffer.is_empty() {
            self.did_stream = true;
            std::mem::take(&mut self.buffer)
        } else {
            vec![]
        }
    }

    /// Abort the pipeline — no more payloads accepted or flushed.
    pub fn stop(&mut self) {
        self.aborted = true;
    }

    pub fn has_buffered(&self) -> bool {
        !self.buffer.is_empty()
    }

    pub fn did_stream(&self) -> bool {
        self.did_stream
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
}
