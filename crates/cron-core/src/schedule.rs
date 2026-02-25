use crate::types::CronScheduleKind;

/// Compute the next run time in milliseconds for a given schedule.
pub fn compute_next_run(schedule: &CronScheduleKind, now_ms: u64) -> Option<u64> {
    match schedule {
        CronScheduleKind::At { at } => {
            let dt = chrono::DateTime::parse_from_rfc3339(at).ok()?;
            let ts = dt.timestamp_millis() as u64;
            if ts > now_ms { Some(ts) } else { None }
        }
        CronScheduleKind::Every { every_ms, anchor_ms } => {
            let anchor = anchor_ms.unwrap_or(0);
            if *every_ms == 0 {
                return None;
            }
            let elapsed = now_ms.saturating_sub(anchor);
            let periods = elapsed / every_ms;
            let next = anchor + (periods + 1) * every_ms;
            // Guard against same-second rescheduling
            if next <= now_ms { Some(next + every_ms) } else { Some(next) }
        }
        CronScheduleKind::Cron { expr, tz: _ } => {
            use cron::Schedule;
            use std::str::FromStr;
            let sched = Schedule::from_str(expr).ok()?;
            let now_dt = chrono::DateTime::from_timestamp_millis(now_ms as i64)?;
            let next = sched.after(&now_dt).next()?;
            Some(next.timestamp_millis() as u64)
        }
    }
}
