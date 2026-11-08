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
