/// Exponential backoff steps in milliseconds: 30s → 60s → 5m → 15m → 60m
const BACKOFF_STEPS: &[u64] = &[30_000, 60_000, 300_000, 900_000, 3_600_000];

/// Returns the backoff delay in milliseconds for the given number of consecutive errors.
pub fn backoff_delay_ms(consecutive_errors: u32) -> u64 {
    if consecutive_errors == 0 {
        return 0;
    }
    let idx = (consecutive_errors as usize)
        .saturating_sub(1)
        .min(BACKOFF_STEPS.len() - 1);
    BACKOFF_STEPS[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_steps() {
        assert_eq!(backoff_delay_ms(0), 0);
        assert_eq!(backoff_delay_ms(1), 30_000);
        assert_eq!(backoff_delay_ms(2), 60_000);
        assert_eq!(backoff_delay_ms(3), 300_000);
        assert_eq!(backoff_delay_ms(4), 900_000);
        assert_eq!(backoff_delay_ms(5), 3_600_000);
        // Clamps at max
        assert_eq!(backoff_delay_ms(100), 3_600_000);
    }
}
