//! Own process: sets TZ, which the libc time functions read.
use hop_bar::spendhist::{self, Tz};

#[test]
fn local_tz_follows_the_process_zone() {
    // POSIX form, no zoneinfo needed: five hours east of UTC
    std::env::set_var("TZ", "XXX-5");
    // 2026-11-07T20:00:00Z is already the 8th at UTC+5
    let t = 1_794_052_800 + 8 * 3600;
    assert_eq!(spendhist::date_of(t, Tz::Utc), "2026-11-07");
    assert_eq!(spendhist::date_of(t, Tz::Local), "2026-11-08");
    assert_eq!(hop_bar::tz::local_offset(t), 5 * 3600);
}
