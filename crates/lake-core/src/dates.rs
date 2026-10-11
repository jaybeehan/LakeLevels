//! Calendar helpers. A date is a whole day number: days since 1970-01-01 (UTC),
//! the same numbers the page's JavaScript uses.

/// Day number for a calendar date. Like JavaScript's `Date.UTC`, a day past the
/// end of the month rolls into the next month (Feb 30 -> Mar 1 or 2).
pub fn day(y: i64, m: u32, d: u32) -> i64 {
    // move to a year that starts in March so the leap day is last
    let (y, m) = if m <= 2 { (y - 1, m as i64 + 9) } else { (y, m as i64 - 3) };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * m + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month, day) for a day number.
pub fn ymd(t: i64) -> (i64, u32, u32) {
    let z = t + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

pub fn year_of(t: i64) -> i64 {
    ymd(t).0
}

pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `k` months from (y, m, d), keeping the day but clamping it to the month's length.
pub fn add_months(y: i64, m: u32, d: u32, k: i64) -> i64 {
    let mm = m as i64 - 1 + k;
    let yy = y + mm.div_euclid(12);
    let mm = (mm.rem_euclid(12) + 1) as u32;
    day(yy, mm, d.min(days_in_month(yy, mm)))
}

/// "YYYY-MM-DD"
pub fn iso(t: i64) -> String {
    let (y, m, d) = ymd(t);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Day number from text starting "YYYY-MM-DD".
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some(day(y, m, d))
}

/// Seconds since 1970 (UTC) from an ISO date-time such as "2026-10-01T07:00:00Z",
/// "2026-10-01 07:00:00", "2026-10-01T00:00:00-07:00" or with fractional seconds.
/// Text without a time zone is taken as UTC.
pub fn parse_datetime(s: &str) -> Option<f64> {
    let s = s.trim();
    let t = parse_iso(s)?;
    let rest = &s[10..];
    if rest.is_empty() {
        return Some(t as f64 * 86_400.0);
    }
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' '))?;
    // split off the zone
    let (clock, zone) = match rest.find(|c| c == 'Z' || c == 'z' || c == '+' || c == '-') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut parts = clock.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let mi: f64 = parts.next().unwrap_or("0").parse().ok()?;
    let sec: f64 = parts.next().unwrap_or("0").parse().ok()?;
    let mut secs = t as f64 * 86_400.0 + h * 3600.0 + mi * 60.0 + sec;
    if let Some(off) = zone.strip_prefix('+').map(|z| (1.0, z)).or_else(|| zone.strip_prefix('-').map(|z| (-1.0, z))) {
        let (sign, z) = off;
        let z = z.replace(':', "");
        if z.len() < 2 {
            return None;
        }
        let oh: f64 = z.get(0..2)?.parse().ok()?;
        let om: f64 = z.get(2..4).unwrap_or("0").parse().unwrap_or(0.0);
        secs -= sign * (oh * 3600.0 + om * 60.0);
    }
    Some(secs)
}

/// Excel's serial day number for a date (1900 date system).
pub fn excel_serial(t: i64) -> f64 {
    (t - day(1899, 12, 30)) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for t in day(1600, 1, 1)..day(2400, 1, 1) {
            let (y, m, d) = ymd(t);
            assert_eq!(day(y, m, d), t);
        }
        assert_eq!(day(1970, 1, 1), 0);
        assert_eq!(iso(day(2024, 2, 29)), "2024-02-29");
        assert_eq!(day(2023, 2, 29), day(2023, 3, 1)); // rolls over like Date.UTC
    }

    #[test]
    fn months() {
        assert_eq!(iso(add_months(2024, 3, 31, -1)), "2024-02-29");
        assert_eq!(iso(add_months(2024, 1, 15, -13)), "2022-12-15");
        assert_eq!(iso(add_months(2024, 11, 30, 3)), "2025-02-28");
    }

    #[test]
    fn datetimes() {
        assert_eq!(parse_datetime("1970-01-02T00:00:00Z"), Some(86_400.0));
        assert_eq!(parse_datetime("1970-01-01T07:00:00-07:00"), Some(14.0 * 3600.0));
        assert_eq!(parse_datetime("1970-01-01 01:30:00"), Some(5400.0));
        assert_eq!(parse_datetime("1970-01-01T00:00:00.5+00:00"), Some(0.5));
        assert_eq!(excel_serial(day(2026, 1, 1)), 46023.0);
    }
}
