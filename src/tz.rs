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
    month_bounds_by(ts, |t| tz.offset(t))
}

/// `month_bounds` with the zone as a function from an instant to its
/// offset.
pub fn month_bounds_by(ts: i64, offset: impl Fn(i64) -> i64) -> (i64, i64) {
    let (y, m, _) = civil((ts + offset(ts)).div_euclid(86_400));
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let at = |days: i64| local_midnight(days * 86_400, &offset);
    (at(days_from_civil(y, m, 1)), at(days_from_civil(ny, nm, 1)))
}

/// The instant whose local time is `local` (seconds of the local wall
/// clock since the epoch): the offset is taken again at the first guess,
/// so a change of offset between `local` read as UTC and the real instant
/// (daylight saving starting that day) does not move it by an hour.
fn local_midnight(local: i64, offset: &impl Fn(i64) -> i64) -> i64 {
    let guess = local - offset(local);
    local - offset(guess)
}

/// `2026-11-09T12:00:00.123456+00:00` or `...Z` (an RFC 3339 time with a
/// zone offset) to unix seconds.
pub fn rfc3339(s: &str) -> Option<i64> {
    let base = crate::transcript::parse_ts(s)?;
    let tail = &s[19..];
    let tail = tail.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let off = match tail.as_bytes() {
        [] | [b'Z'] => 0,
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] => {
            let d = |a: &u8, b: &u8| -> Option<i64> {
                (a.is_ascii_digit() && b.is_ascii_digit())
                    .then(|| i64::from((a - b'0') * 10 + (b - b'0')))
            };
            let o = d(h1, h2)? * 3600 + d(m1, m2)? * 60;
            if *sign == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    Some(base - off)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sydney 2023: AEST (+10) until 2023-10-01 02:00 local, then AEDT (+11).
    fn sydney(ts: i64) -> i64 {
        let dst = days_from_civil(2023, 10, 1) * 86_400 + 2 * 3600 - 10 * 3600;
        if ts >= dst {
            11 * 3600
        } else {
            10 * 3600
        }
    }

    #[test]
    fn month_starts_on_a_daylight_saving_day() {
        // 2023-10-15 noon local: October starts at local midnight, still AEST
        let ts = days_from_civil(2023, 10, 15) * 86_400;
        let (start, _) = month_bounds_by(ts, sydney);
        assert_eq!(start, days_from_civil(2023, 10, 1) * 86_400 - 10 * 3600);
        // and September ends there
        let sep = days_from_civil(2023, 9, 20) * 86_400;
        assert_eq!(month_bounds_by(sep, sydney).1, start);
    }

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
