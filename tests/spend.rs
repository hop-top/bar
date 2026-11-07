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
            period: period.into(),
            resets_at: NOW + left,
        }),
        ..Session::default()
    }
}

#[test]
fn monthly_shows_dim_time_left() {
    let out = go(&limit("monthly", 3 * 86400 + 4 * 3600)).unwrap();
    assert_eq!(out, "\x1b[2m3d4h\x1b[22m");
}

#[test]
fn other_period_past_reset_or_absent_is_omitted() {
    assert_eq!(go(&limit("weekly", 86400)), None);
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
            period: "monthly".into(),
            resets_at: 1234
        })
    );
    assert_eq!(session::parse(r#"{"rate_limits":{}}"#).spend_limit, None);
    let s = session::parse(r#"{"rate_limits":{"spend_limit":{"period":"monthly"}}}"#);
    assert_eq!(s.spend_limit, None);
}
