use std::collections::HashMap;

const BACKOFF_SCHEDULE: &[u64] = &[5000, 10000, 30000, 60000];

#[derive(Debug, Clone)]
struct PollState {
    count: usize,
    last_poll_at: u64,
}

#[derive(Default)]
pub struct CommandPollTracker {
    polls: HashMap<String, PollState>,
}

impl CommandPollTracker {
    pub fn record(&mut self, command_id: &str, has_new_output: bool, now_ms: u64) -> u64 {
        let state = self
            .polls
            .entry(command_id.to_string())
            .or_insert(PollState {
                count: 0,
                last_poll_at: now_ms,
            });
        state.last_poll_at = now_ms;
        if has_new_output {
            state.count = 0;
        } else {
            state.count += 1;
        }
        backoff_ms(state.count)
    }

    pub fn suggestion(&self, command_id: &str) -> u64 {
        self.polls
            .get(command_id)
            .map_or(BACKOFF_SCHEDULE[0], |s| backoff_ms(s.count))
    }

    pub fn reset(&mut self, command_id: &str) {
        self.polls.remove(command_id);
    }

    /// Remove polls older than `max_age_ms` (default 1 hour).
    pub fn prune_stale(&mut self, now_ms: u64, max_age_ms: u64) {
        self.polls
            .retain(|_, s| now_ms.saturating_sub(s.last_poll_at) < max_age_ms);
    }
}

fn backoff_ms(consecutive_no_output: usize) -> u64 {
    let idx = consecutive_no_output.min(BACKOFF_SCHEDULE.len() - 1);
    BACKOFF_SCHEDULE[idx]
}
