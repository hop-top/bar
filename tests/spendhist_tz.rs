//! Own process: sets TZ, which the libc time functions read on Unix and
//! which Windows ignores in favour of the system zone.
use hop_bar::spendhist::{self, Tz};

// 2026-11-07T20:00:00Z: already the 8th at UTC+5
const T: i64 = 1_794_052_800 + 8 * 3600;

#[cfg(unix)]
#[test]
fn local_tz_follows_the_process_zone() {
    // POSIX form, no zoneinfo needed: five hours east of UTC
    std::env::set_var("TZ", "XXX-5");
    assert_eq!(spendhist::date_of(T, Tz::Utc), "2026-11-07");
    assert_eq!(spendhist::date_of(T, Tz::Local), "2026-11-08");
    assert_eq!(hop_bar::tz::local_offset(T), 5 * 3600);
}

#[cfg(windows)]
#[test]
fn local_tz_is_the_system_zone_not_tz() {
    let system = hop_bar::tz::local_offset(T);
    std::env::set_var("TZ", "XXX-5");
    assert_eq!(hop_bar::tz::local_offset(T), system);
    assert_eq!(spendhist::date_of(T, Tz::Utc), "2026-11-07");
    let want = hop_bar::tz::day_string(hop_bar::tz::day_of(T + system, Tz::Utc));
    assert_eq!(spendhist::date_of(T, Tz::Local), want);
}
