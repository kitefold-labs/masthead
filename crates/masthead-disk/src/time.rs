// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Timestamps: the current time, RFC 3339 parsing for Docker's dates, and
//! short ages for display.

use std::time::{SystemTime, UNIX_EPOCH};

pub const DAY: i64 = 86_400;

/// Seconds since the epoch.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Parses `2026-10-02T14:03:11.123456789Z` or with a `+hh:mm` offset.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest.as_bytes().first() {
        Some(b'Z' | b'z') | None => 0,
        Some(sign @ (b'+' | b'-')) => {
            let oh = rest.get(1..3)?.parse::<i64>().ok()?;
            let om = rest.get(4..6)?.parse::<i64>().ok()?;
            let o = oh * 3600 + om * 60;
            if *sign == b'+' { o } else { -o }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * DAY + h * 3600 + mi * 60 + se - offset)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "3 days", "5 months", "2 years" since `then`.
pub fn age(then: i64, now: i64) -> String {
    let s = (now - then).max(0);
    let (n, unit) = match s {
        s if s < 3600 => (s / 60, "minute"),
        s if s < DAY => (s / 3600, "hour"),
        s if s < 60 * DAY => (s / DAY, "day"),
        s if s < 730 * DAY => (s / (30 * DAY), "month"),
        s => (s / (365 * DAY), "year"),
    };
    if n == 1 { format!("1 {unit}") } else { format!("{n} {unit}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-11-06T12:00:00.123456789Z"), Some(1_793_966_400));
        assert_eq!(parse_rfc3339("2026-11-06T13:00:00+01:00"), Some(1_793_966_400));
        assert_eq!(parse_rfc3339("0001-01-01T00:00:00Z"), Some(-62_135_596_800));
        assert_eq!(parse_rfc3339("yesterday"), None);
    }

    #[test]
    fn ages() {
        assert_eq!(age(0, 59), "0 minutes");
        assert_eq!(age(0, 3 * DAY), "3 days");
        assert_eq!(age(0, 95 * DAY), "3 months");
        assert_eq!(age(0, 800 * DAY), "2 years");
        assert_eq!(age(0, 2 * 3600), "2 hours");
    }
}
