use std::collections::VecDeque;

/// Detects repetitive tool-call patterns in the agent loop.
pub struct LoopDetector {
    window: VecDeque<String>,
    window_size: usize,
    max_repeats: usize,
}

impl LoopDetector {
    pub fn new(window_size: usize, max_repeats: usize) -> Self {
        Self {
            window: VecDeque::new(),
            window_size,
            max_repeats,
        }
    }

    /// Record a tool call signature and return true if a loop is detected.
    pub fn record(&mut self, tool_name: &str, args: &str) -> bool {
        let sig = format!("{}:{}", tool_name, args);
        self.window.push_back(sig.clone());
        if self.window.len() > self.window_size {
            self.window.pop_front();
        }
        // Count how many times this exact signature appears in the window
        let count = self.window.iter().filter(|s| *s == &sig).count();
        count >= self.max_repeats
    }

    pub fn reset(&mut self) {
        self.window.clear();
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new(10, 3)
    }
}
