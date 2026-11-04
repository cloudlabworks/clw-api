//! Civil dates as days since 1970-01-01 (UTC), without a date crate.
//!
//! The conversions are Howard Hinnant's `days_from_civil` and `civil_from_days`.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Parse a leading `YYYY-MM-DD` (anything after the tenth byte is ignored, so
/// `2026-11-03T10:00:00Z` works). Rejects impossible dates such as `2026-02-30`.
pub fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = s.get(r)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let (y, m, d) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(i64::from(y), m, d);
    (civil_from_days(days) == (i64::from(y), m, d)).then_some(days)
}

pub fn format_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn from_system_time(t: SystemTime) -> Option<i64> {
    let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs / 86_400).ok()
}

/// Today in UTC, as days since 1970-01-01.
pub fn today() -> i64 {
    from_system_time(SystemTime::now()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(parse_date("2000-03-01"), Some(11_017));
        assert_eq!(format_date(11_017), "2000-03-01");
        for days in [-1000, 0, 365, 19_000, 20_760, 30_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn parse_rejects_bad_dates() {
        assert!(parse_date("2026-02-30").is_none());
        assert!(parse_date("2026-13-01").is_none());
        assert!(parse_date("2026-1-01").is_none());
        assert!(parse_date("20x6-01-01").is_none());
        assert!(parse_date("2024-02-29").is_some());
        assert_eq!(parse_date("2026-11-03T10:00:00Z"), parse_date("2026-11-03"));
    }
}
