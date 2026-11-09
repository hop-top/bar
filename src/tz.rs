//! Local-time offsets and civil dates without a date-time dependency.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tz {
    Utc,
    Local,
}

impl Tz {
    pub fn name(self) -> &'static str {
        match self {
            Tz::Utc => "utc",
            Tz::Local => "local",
        }
    }

    /// Seconds east of UTC at the instant `ts`.
    pub fn offset(self, ts: i64) -> i64 {
        match self {
            Tz::Utc => 0,
            Tz::Local => local_offset(ts),
        }
    }
}

/// The local zone at `ts`, in seconds east of UTC: `TZ`, else the
/// system's, on Unix; the system zone on Windows. 0 when it cannot be
/// converted.
pub fn local_offset(ts: i64) -> i64 {
    crate::platform::local_offset(ts)
}

/// Days since 1970-01-01 to `(year, month, day)` (Hinnant).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `YYYY-MM-DD` for a day number.
pub fn day_string(days: i64) -> String {
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Local day number of `ts`.
pub fn day_of(ts: i64, tz: Tz) -> i64 {
    (ts + tz.offset(ts)).div_euclid(86_400)
}

/// `(year, month, day)` to days since 1970-01-01 (Hinnant).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let (m, d) = (i64::from(m), i64::from(d));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of the start of the calendar month holding `ts` and of
/// the next one, in `tz`.
pub fn month_bounds(ts: i64, tz: Tz) -> (i64, i64) {
    let (y, m, _) = civil(day_of(ts, tz));
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let at = |days: i64| {
        let local = days * 86_400;
        local - tz.offset(local)
    };
    (at(days_from_civil(y, m, 1)), at(days_from_civil(ny, nm, 1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trips_and_months() {
        for days in [-1, 0, 59, 10_957, 20_401, 20_454] {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        // 2026-11-09T12:00Z: November 2026, then December
        let ts = days_from_civil(2026, 11, 9) * 86_400 + 43_200;
        let (a, b) = month_bounds(ts, Tz::Utc);
        assert_eq!(a, days_from_civil(2026, 11, 1) * 86_400);
        assert_eq!(b, days_from_civil(2026, 12, 1) * 86_400);
        let dec = days_from_civil(2026, 12, 31) * 86_400;
        assert_eq!(
            month_bounds(dec, Tz::Utc).1,
            days_from_civil(2027, 1, 1) * 86_400
        );
    }
}
