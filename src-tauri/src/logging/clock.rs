//! Wall-clock helpers for the logging layer.
//!
//! Two things need the time here and they need it in different shapes: a log
//! file is stamped with human-readable UTC, and a log path is built from whole
//! seconds (`crate::config::run_log_path`). Both come through this module so
//! the formatter is written once and a test can pin the format.
//!
//! Nothing here consults the local time zone: the offset a path needs is
//! resolved separately (`crate::config::local_utc_offset_secs`), because a run
//! lives across both and only one of them should be able to move under a test.

use std::time::SystemTime;

/// The instant now.
pub fn now() -> SystemTime {
    SystemTime::now()
}

/// `instant` as RFC 3339 in UTC, e.g. `2026-09-24T01:30:15Z`.
///
/// UTC rather than local time on purpose: a log file outlives the time zone it
/// was written in, and it is the only form that stays comparable between two
/// machines. `None` for an instant before the epoch, which no run can have.
pub fn rfc3339_utc(instant: SystemTime) -> Option<String> {
    use chrono::{DateTime, SecondsFormat};
    let secs = unix_secs(instant)?;
    let utc = DateTime::from_timestamp(secs, 0)?;
    Some(utc.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// Whole seconds since the Unix epoch; `None` for a pre-epoch instant.
pub fn unix_secs(instant: SystemTime) -> Option<i64> {
    instant
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// The fixture instant the path helpers use, so a timestamp that disagreed
    /// with them would show up here too.
    fn start_instant() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_790_213_415)
    }

    #[test]
    fn an_instant_formats_as_rfc_3339_utc() {
        assert_eq!(
            rfc3339_utc(start_instant()),
            Some("2026-09-24T01:30:15Z".to_owned())
        );
    }

    #[test]
    fn unix_seconds_match_the_path_helpers_input() {
        assert_eq!(unix_secs(start_instant()), Some(1_790_213_415));
    }

    /// A pre-epoch instant has no RFC 3339 form to offer; reporting nothing
    /// beats reporting a wrong time.
    #[test]
    fn an_instant_before_the_epoch_has_no_stamp() {
        let before = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(unix_secs(before), None);
        assert_eq!(rfc3339_utc(before), None);
    }

    #[test]
    fn the_clock_reads_the_system_time() {
        let now = now();
        assert!(
            unix_secs(now).unwrap_or(0) > 1_700_000_000,
            "the clock is before the project existed"
        );
    }
}
