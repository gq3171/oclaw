/// Temporal decay: exponential half-life scoring for recency weighting.
#[derive(Debug, Clone)]
pub struct TemporalDecayConfig {
    pub enabled: bool,
    pub half_life_days: f64,
}

impl Default for TemporalDecayConfig {
    fn default() -> Self {
        Self { enabled: false, half_life_days: 30.0 }
    }
}

/// Calculate decay multiplier: e^(-λ * age_days) where λ = ln(2) / half_life_days.
pub fn decay_multiplier(age_days: f64, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 || age_days <= 0.0 {
        return 1.0;
    }
    let lambda = f64::ln(2.0) / half_life_days;
    (-lambda * age_days).exp()
}

/// Apply temporal decay to a score.
pub fn apply_decay(score: f32, age_days: f64, config: &TemporalDecayConfig) -> f32 {
    if !config.enabled {
        return score;
    }
    (score as f64 * decay_multiplier(age_days, config.half_life_days)) as f32
}

/// Extract age in days from a dated path like "memory/2025-01-15.md".
pub fn age_days_from_path(path: &str, now_epoch_secs: i64) -> Option<f64> {
    // Look for YYYY-MM-DD pattern
    let re_like = path.chars().collect::<Vec<_>>();
    for window in re_like.windows(10) {
        let s: String = window.iter().collect();
        if s.len() == 10
            && s.as_bytes()[4] == b'-'
            && s.as_bytes()[7] == b'-'
            && s[..4].chars().all(|c| c.is_ascii_digit())
            && s[5..7].chars().all(|c| c.is_ascii_digit())
            && s[8..10].chars().all(|c| c.is_ascii_digit())
        {
            // Parse as naive date
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                let file_ts = date.and_hms_opt(0, 0, 0)
                    .map(|dt| dt.and_utc().timestamp())?;
                let age_secs = now_epoch_secs - file_ts;
                return Some(age_secs as f64 / 86400.0);
            }
        }
    }
    None
}
