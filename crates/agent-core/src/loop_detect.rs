use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const HISTORY_SIZE: usize = 30;
const WARNING_THRESHOLD: usize = 10;
const CRITICAL_THRESHOLD: usize = 20;
const CIRCUIT_BREAKER_THRESHOLD: usize = 30;

/// Known polling tool calls that get special no-progress detection.
fn is_known_poll_tool(name: &str, args: &str) -> bool {
    if name == "command_status" {
        return true;
    }
    if name == "process" {
        return args.contains("\"poll\"") || args.contains("\"log\"");
    }
    false
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopLevel {
    None,
    Warning,
    Critical,
}

#[derive(Debug, Clone)]
pub struct LoopDetectionResult {
    pub level: LoopLevel,
    pub message: Option<String>,
}

impl LoopDetectionResult {
    fn ok() -> Self {
        Self {
            level: LoopLevel::None,
            message: None,
        }
    }
    fn warn(msg: String) -> Self {
        Self {
            level: LoopLevel::Warning,
            message: Some(msg),
        }
    }
    fn critical(msg: String) -> Self {
        Self {
            level: LoopLevel::Critical,
            message: Some(msg),
        }
    }
}

#[derive(Clone)]
struct ToolCallRecord {
    tool_name: String,
    args_hash: u64,
    result_hash: Option<u64>,
}

/// Detects repetitive tool-call patterns matching Node openclaw's 4-detector design:
/// generic_repeat, known_poll_no_progress, ping_pong, global_circuit_breaker.
pub struct LoopDetector {
    history: Vec<ToolCallRecord>,
    history_size: usize,
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            history_size: HISTORY_SIZE,
        }
    }
}

impl LoopDetector {
    /// Record a tool call into the sliding window history.
    pub fn record(&mut self, tool_name: &str, args: &str) {
        let args_hash = hash_str(&format!("{}:{}", tool_name, args));
        self.history.push(ToolCallRecord {
            tool_name: tool_name.to_string(),
            args_hash,
            result_hash: None,
        });
        if self.history.len() > self.history_size {
            self.history.remove(0);
        }
    }

    /// Record the outcome (result hash) of the most recent tool call.
    pub fn record_outcome(&mut self, result: &str) {
        if let Some(last) = self.history.last_mut() {
            last.result_hash = Some(hash_str(result));
        }
    }

    /// Run all 4 detectors against the current tool call. Call after `record()`.
    pub fn detect(&self, tool_name: &str, args: &str) -> LoopDetectionResult {
        let current_hash = hash_str(&format!("{}:{}", tool_name, args));
        let is_poll = is_known_poll_tool(tool_name, args);

        // 1. Global circuit breaker — no-progress streak across all calls
        let no_progress = self.no_progress_streak(tool_name, current_hash);
        if no_progress >= CIRCUIT_BREAKER_THRESHOLD {
            return LoopDetectionResult::critical(format!(
                "CRITICAL: {} has repeated identical no-progress outcomes {} times. \
                 Session execution blocked by global circuit breaker.",
                tool_name, no_progress
            ));
        }

        // 2. Known poll no-progress
        if is_poll && no_progress >= CRITICAL_THRESHOLD {
            return LoopDetectionResult::critical(format!(
                "CRITICAL: Called {} with identical arguments and no progress {} times. \
                 This appears to be a stuck polling loop.",
                tool_name, no_progress
            ));
        }
        if is_poll && no_progress >= WARNING_THRESHOLD {
            return LoopDetectionResult::warn(format!(
                "WARNING: You have called {} {} times with identical arguments and no progress. \
                 Stop polling and either increase wait time or report the task as failed.",
                tool_name, no_progress
            ));
        }

        // 3. Ping-pong detection
        let pp = self.ping_pong_streak(current_hash);
        if pp.count >= CRITICAL_THRESHOLD && pp.no_progress {
            return LoopDetectionResult::critical(format!(
                "CRITICAL: You are alternating between repeated tool-call patterns \
                 ({} consecutive calls) with no progress. Stuck ping-pong loop detected.",
                pp.count
            ));
        }
        if pp.count >= WARNING_THRESHOLD {
            return LoopDetectionResult::warn(format!(
                "WARNING: You are alternating between repeated tool-call patterns \
                 ({} consecutive calls). This looks like a ping-pong loop; \
                 stop retrying and report the task as failed.",
                pp.count
            ));
        }

        // 4. Generic repeat — identical calls in window
        if !is_poll {
            let repeat_count = self
                .history
                .iter()
                .filter(|r| r.tool_name == tool_name && r.args_hash == current_hash)
                .count();
            if repeat_count >= WARNING_THRESHOLD {
                return LoopDetectionResult::warn(format!(
                    "WARNING: You have called {} {} times with identical arguments. \
                     If this is not making progress, stop retrying and report the task as failed.",
                    tool_name, repeat_count
                ));
            }
        }

        LoopDetectionResult::ok()
    }

    /// Count consecutive no-progress calls (same tool+args+result) from the tail.
    /// Breaks on any non-matching record to ensure only truly consecutive calls are counted.
    fn no_progress_streak(&self, tool_name: &str, args_hash: u64) -> usize {
        let mut streak = 0usize;
        let mut expected_result: Option<u64> = None;
        for rec in self.history.iter().rev() {
            if rec.tool_name != tool_name || rec.args_hash != args_hash {
                break; // Stop at first non-matching record — streak must be consecutive
            }
            let Some(rh) = rec.result_hash else { break };
            match expected_result {
                None => {
                    expected_result = Some(rh);
                    streak = 1;
                }
                Some(exp) if rh == exp => streak += 1,
                _ => break,
            }
        }
        streak
    }

    /// Detect A-B-A-B alternating pattern at the tail of history.
    fn ping_pong_streak(&self, current_hash: u64) -> PingPongResult {
        let empty = PingPongResult {
            count: 0,
            no_progress: false,
        };
        let Some(last) = self.history.last() else {
            return empty;
        };

        // Find the "other" signature (first different one scanning backwards)
        let mut other_hash: Option<u64> = None;
        for rec in self.history.iter().rev().skip(1) {
            if rec.args_hash != last.args_hash {
                other_hash = Some(rec.args_hash);
                break;
            }
        }
        let Some(other) = other_hash else {
            return empty;
        };

        // Count alternating tail
        let mut alt_count = 0usize;
        for rec in self.history.iter().rev() {
            let expected = if alt_count.is_multiple_of(2) {
                last.args_hash
            } else {
                other
            };
            if rec.args_hash != expected {
                break;
            }
            alt_count += 1;
        }
        if alt_count < 2 {
            return empty;
        }

        // Current call must continue the pattern
        if current_hash != other {
            return empty;
        }

        // Check no-progress: all results for each side must be identical
        let tail_start = self.history.len().saturating_sub(alt_count);
        let mut hash_a: Option<u64> = None;
        let mut hash_b: Option<u64> = None;
        let mut no_progress = true;
        for rec in &self.history[tail_start..] {
            let Some(rh) = rec.result_hash else {
                no_progress = false;
                break;
            };
            if rec.args_hash == last.args_hash {
                match hash_a {
                    None => hash_a = Some(rh),
                    Some(h) if h != rh => {
                        no_progress = false;
                        break;
                    }
                    _ => {}
                }
            } else {
                match hash_b {
                    None => hash_b = Some(rh),
                    Some(h) if h != rh => {
                        no_progress = false;
                        break;
                    }
                    _ => {}
                }
            }
        }
        if hash_a.is_none() || hash_b.is_none() {
            no_progress = false;
        }

        PingPongResult {
            count: alt_count + 1,
            no_progress,
        }
    }
}

struct PingPongResult {
    count: usize,
    no_progress: bool,
}
