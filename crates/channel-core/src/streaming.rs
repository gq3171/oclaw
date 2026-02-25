use std::time::Instant;

/// Coalesces streaming chunks into larger batches before flushing,
/// reducing the number of message edits sent to a channel.
pub struct StreamCoalescer {
    min_chars: usize,
    idle_ms: u64,
    buffer: String,
    last_flush: Instant,
}

impl StreamCoalescer {
    pub fn new(min_chars: usize, idle_ms: u64) -> Self {
        Self {
            min_chars,
            idle_ms,
            buffer: String::new(),
            last_flush: Instant::now(),
        }
    }

    /// Push a chunk into the buffer.
    /// Returns `Some(text)` when the buffer should be flushed.
    pub fn push(&mut self, chunk: &str) -> Option<String> {
        self.buffer.push_str(chunk);
        let elapsed = self.last_flush.elapsed().as_millis() as u64;
        if self.buffer.len() >= self.min_chars || elapsed >= self.idle_ms {
            self.do_flush()
        } else {
            None
        }
    }

    /// Force-flush whatever is in the buffer.
    pub fn flush(&mut self) -> Option<String> {
        self.do_flush()
    }

    fn do_flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }
        let out = std::mem::take(&mut self.buffer);
        self.last_flush = Instant::now();
        Some(out)
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

impl Default for StreamCoalescer {
    fn default() -> Self {
        Self::new(1500, 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_on_min_chars() {
        let mut c = StreamCoalescer::new(10, 60_000);
        assert!(c.push("hello").is_none());
        let out = c.push(" world!!");
        assert!(out.is_some());
        assert_eq!(out.unwrap(), "hello world!!");
    }

    #[test]
    fn force_flush() {
        let mut c = StreamCoalescer::new(1000, 60_000);
        c.push("partial");
        let out = c.flush();
        assert_eq!(out.unwrap(), "partial");
        assert!(c.flush().is_none());
    }

    #[test]
    fn empty_flush_returns_none() {
        let mut c = StreamCoalescer::default();
        assert!(c.flush().is_none());
    }
}
