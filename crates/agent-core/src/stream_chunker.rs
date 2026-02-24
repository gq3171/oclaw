/// Streaming soft chunker with fence-aware splitting and break preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakPreference {
    Paragraph,
    Newline,
    Sentence,
}

#[derive(Debug, Clone)]
pub struct ChunkingConfig {
    pub min_chars: usize,
    pub max_chars: usize,
    pub break_preference: BreakPreference,
    pub flush_on_paragraph: bool,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            min_chars: 200,
            max_chars: 1500,
            break_preference: BreakPreference::Paragraph,
            flush_on_paragraph: false,
        }
    }
}

struct FenceSpan {
    start: usize,
    end: usize,
    open_line: String,
    marker: char,
    indent: String,
}

pub struct StreamChunker {
    config: ChunkingConfig,
    buffer: String,
}

impl StreamChunker {
    pub fn new(config: ChunkingConfig) -> Self {
        Self { config, buffer: String::new() }
    }

    pub fn push(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    /// Drain available chunks. If `force`, flush remaining buffer.
    pub fn drain(&mut self, force: bool) -> Vec<String> {
        let mut chunks = Vec::new();
        loop {
            if self.buffer.trim().is_empty() {
                if force { self.buffer.clear(); }
                break;
            }
            if !force && self.buffer.len() < self.config.min_chars {
                break;
            }
            if force && self.buffer.len() <= self.config.max_chars {
                let chunk = self.buffer.trim().to_string();
                self.buffer.clear();
                if !chunk.is_empty() { chunks.push(chunk); }
                break;
            }
            if let Some(idx) = self.pick_break_index() {
                let (chunk, fence_close, fence_reopen) = self.split_at(idx);
                let mut out = chunk;
                if let Some(close) = fence_close { out.push_str(&close); }
                let trimmed = out.trim().to_string();
                if !trimmed.is_empty() { chunks.push(trimmed); }
                if let Some(reopen) = fence_reopen {
                    self.buffer = format!("{}{}", reopen, self.buffer.trim_start_matches('\n'));
                }
            } else {
                break;
            }
        }
        chunks
    }

    fn pick_break_index(&self) -> Option<usize> {
        let window = self.buffer.len().min(self.config.max_chars);
        let buf = &self.buffer[..window];
        let spans = parse_fence_spans(buf);

        // Try preferred break type first, then fallback chain
        match self.config.break_preference {
            BreakPreference::Paragraph => {
                self.find_paragraph_break(buf, &spans)
                    .or_else(|| self.find_newline_break(buf, &spans))
                    .or_else(|| self.find_sentence_break(buf, &spans))
            }
            BreakPreference::Newline => {
                self.find_newline_break(buf, &spans)
                    .or_else(|| self.find_sentence_break(buf, &spans))
            }
            BreakPreference::Sentence => {
                self.find_sentence_break(buf, &spans)
            }
        }
        .or(Some(window)) // hard break at max_chars
    }

    fn find_paragraph_break(&self, buf: &str, spans: &[FenceSpan]) -> Option<usize> {
        // Search backwards for \n\n
        let mut i = buf.len();
        while i > self.config.min_chars {
            if let Some(pos) = buf[..i].rfind("\n\n")
                && pos >= self.config.min_chars && is_safe_fence_break(spans, pos)
            {
                return Some(pos);
            }
            i = i.saturating_sub(1);
            if i <= self.config.min_chars { break; }
        }
        None
    }

    fn find_newline_break(&self, buf: &str, spans: &[FenceSpan]) -> Option<usize> {
        let mut i = buf.len();
        while i > self.config.min_chars {
            if let Some(pos) = buf[..i].rfind('\n')
                && pos >= self.config.min_chars && is_safe_fence_break(spans, pos)
            {
                return Some(pos);
            }
            i = i.saturating_sub(1);
            if i <= self.config.min_chars { break; }
        }
        None
    }

    fn find_sentence_break(&self, buf: &str, spans: &[FenceSpan]) -> Option<usize> {
        let mut last = None;
        for (i, c) in buf.char_indices() {
            if i < self.config.min_chars { continue; }
            if matches!(c, '.' | '!' | '?') {
                let next = i + c.len_utf8();
                let at_end = next >= buf.len();
                let followed_by_space = buf[next..].starts_with(|c: char| c.is_whitespace());
                if (at_end || followed_by_space) && is_safe_fence_break(spans, next) {
                    last = Some(next);
                }
            }
        }
        last
    }

    /// Split buffer at index, handling fence boundaries.
    fn split_at(&mut self, idx: usize) -> (String, Option<String>, Option<String>) {
        let chunk = self.buffer[..idx].to_string();
        self.buffer = self.buffer[idx..].to_string();
        // Strip leading newlines from remaining buffer
        let trimmed = self.buffer.trim_start_matches('\n').to_string();
        self.buffer = trimmed;

        let spans = parse_fence_spans(&chunk);
        // Check if we split inside an unclosed fence
        if let Some(span) = spans.last()
            && span.end >= chunk.len()
        {
            let close = format!("\n{}{}", span.indent, span.marker.to_string().repeat(3));
            let reopen = format!("{}\n", span.open_line);
            return (chunk, Some(close), Some(reopen));
        }
        (chunk, None, None)
    }
}

fn parse_fence_spans(text: &str) -> Vec<FenceSpan> {
    let mut spans = Vec::new();
    let mut open: Option<(usize, char, usize, String, String)> = None; // (start, marker_char, marker_len, indent, open_line)
    let mut pos = 0;

    for line in text.split('\n') {
        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        if indent_len <= 3 {
            let marker_char = trimmed.chars().next().unwrap_or(' ');
            if matches!(marker_char, '`' | '~') {
                let marker_len = trimmed.chars().take_while(|&c| c == marker_char).count();
                if marker_len >= 3 {
                    let indent = &line[..indent_len];
                    if let Some((start, open_marker, open_len, ref open_indent, ref open_line)) = open {
                        if marker_char == open_marker && marker_len >= open_len {
                            spans.push(FenceSpan {
                                start,
                                end: pos + line.len(),
                                open_line: open_line.clone(),
                                marker: open_marker,
                                indent: open_indent.clone(),
                            });
                            open = None;
                        }
                    } else {
                        open = Some((pos, marker_char, marker_len, indent.to_string(), line.to_string()));
                    }
                }
            }
        }
        pos += line.len() + 1; // +1 for \n
    }

    // Unclosed fence extends to end
    if let Some((start, marker, _len, indent, open_line)) = open {
        spans.push(FenceSpan {
            start,
            end: text.len(),
            open_line,
            marker,
            indent,
        });
    }

    spans
}

fn is_safe_fence_break(spans: &[FenceSpan], index: usize) -> bool {
    !spans.iter().any(|s| index > s.start && index < s.end)
}
