//! Local-time offsets and civil dates without a date-time dependency.

use std::ffi::{c_char, c_long};

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

/// Mirrors `struct tm` on 64-bit macOS and Linux (glibc, musl).
#[repr(C)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
    gmtoff: c_long,
    zone: *const c_char,
}

extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

/// The process's zone (`TZ`, else the system's) at `ts`, in seconds east
/// of UTC; 0 when the C library cannot convert it.
pub fn local_offset(ts: i64) -> i64 {
    // SAFETY: `Tm` is plain data for which all-zero is valid, and
    // `localtime_r` writes only into the buffer it is given.
    unsafe {
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&ts, &mut tm).is_null() {
            0
        } else {
            tm.gmtoff
        }
    }
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
