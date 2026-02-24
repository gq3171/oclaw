use std::collections::HashMap;

const MAX_COOLDOWN_MS: u64 = 3_600_000; // 1 hour
const FAILURE_WINDOW_MS: u64 = 24 * 3_600_000; // 24 hours

#[derive(Debug, Clone, Default)]
pub struct ProfileUsageStats {
    pub cooldown_until: Option<u64>,
    pub error_count: u32,
    pub last_failure_at: Option<u64>,
}

#[derive(Default)]
pub struct AuthCooldownTracker {
    profiles: HashMap<String, ProfileUsageStats>,
}

impl AuthCooldownTracker {
    /// Calculate cooldown: 60s * 5^(min(errorCount-1, 3)), capped at 1hr.
    pub fn cooldown_ms(error_count: u32) -> u64 {
        let n = error_count.max(1);
        let exp = (n - 1).min(3);
        let raw = 60_000u64.saturating_mul(5u64.saturating_pow(exp));
        raw.min(MAX_COOLDOWN_MS)
    }

    /// Record a failure for a provider profile.
    pub fn record_failure(&mut self, profile_id: &str, now_ms: u64) -> u64 {
        let stats = self.profiles.entry(profile_id.to_string()).or_default();

        // Reset error count if outside failure window
        if let Some(last) = stats.last_failure_at
            && now_ms.saturating_sub(last) > FAILURE_WINDOW_MS
        {
            stats.error_count = 0;
        }

        stats.error_count += 1;
        stats.last_failure_at = Some(now_ms);
        let cd = Self::cooldown_ms(stats.error_count);
        stats.cooldown_until = Some(now_ms + cd);
        cd
    }

    /// Check if a profile is currently in cooldown.
    pub fn is_cooled_down(&self, profile_id: &str, now_ms: u64) -> bool {
        self.profiles.get(profile_id).is_some_and(|s| {
            s.cooldown_until.is_some_and(|until| now_ms < until)
        })
    }

    /// Clear expired cooldowns and reset error counters.
    pub fn clear_expired(&mut self, now_ms: u64) -> bool {
        let mut mutated = false;
        for stats in self.profiles.values_mut() {
            if let Some(until) = stats.cooldown_until
                && now_ms >= until
            {
                stats.cooldown_until = None;
                stats.error_count = 0;
                stats.last_failure_at = None;
                mutated = true;
            }
        }
        mutated
    }

    pub fn get_stats(&self, profile_id: &str) -> Option<&ProfileUsageStats> {
        self.profiles.get(profile_id)
    }
}
