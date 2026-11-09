use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::spend::render_with;
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::{self, Session, SpendLimit};

const NOW: i64 = 1_000_000;

fn go(s: &Session) -> Option<String> {
    let cfg = Config::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: s,
        config: &cfg,
        now: NOW,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    render_with(&ctx, &Options::default())
}

fn limit(period: &str, left: i64) -> Session {
    Session {
        spend_limit: Some(SpendLimit {
            period: Some(period.into()),
            resets_at: NOW + left,
            ..SpendLimit::default()
        }),
        ..Session::default()
    }
}

#[test]
fn monthly_shows_dim_time_left() {
    let out = go(&limit("monthly", 3 * 86400 + 4 * 3600)).unwrap();
    assert_eq!(out, "\x1b[38;5;250m3d4h\x1b[39m");
}

#[test]
fn unknown_period_past_reset_or_absent_is_omitted() {
    assert_eq!(go(&limit("yearly", 86400)), None);
    assert!(go(&limit("weekly", 86400)).is_some());
    assert!(go(&limit("daily", 3600)).is_some());
    assert_eq!(go(&limit("monthly", -5)), None);
    assert_eq!(go(&limit("monthly", 0)), None);
    assert_eq!(go(&Session::default()), None);
}

#[test]
fn session_parses_spend_limit() {
    let s = session::parse(
        r#"{"rate_limits":{"spend_limit":{"period":"monthly","resets_at":1234,"extra":1}}}"#,
    );
    assert_eq!(
        s.spend_limit,
        Some(SpendLimit {
            period: Some("monthly".into()),
            resets_at: 1234,
            ..SpendLimit::default()
        })
    );
    assert_eq!(session::parse(r#"{"rate_limits":{}}"#).spend_limit, None);
    let s = session::parse(r#"{"rate_limits":{"spend_limit":{"period":"monthly"}}}"#);
    assert_eq!(s.spend_limit, None);
    // the period comes later than the percentage
    let s = session::parse(r#"{"rate_limits":{"spend_limit":{"resets_at":1234}}}"#);
    let l = s.spend_limit.unwrap();
    assert_eq!((l.period, l.resets_at), (None, 1234));
}
