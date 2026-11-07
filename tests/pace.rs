use hop_bar::conf::{Amount, Early, Rule};
use hop_bar::pace::{builtin_rule, score};

const H: i64 = 3600;
const D: i64 = 86400;

fn weekly() -> Rule {
    builtin_rule("7d")
}
fn five() -> Rule {
    builtin_rule("5h")
}

#[test]
fn builtin_rules_match_spec() {
    let r = weekly();
    assert_eq!(r.target, "usage[type=7d]");
    assert_eq!(
        r.early,
        Early::Thresholds {
            warn: Amount::Percent(20),
            bad: Amount::Percent(5)
        }
    );
    assert_eq!(r.final_secs, 86400);
    assert_eq!(r.final_bad, Amount::Percent(30));
    assert_eq!(five().final_secs, 3600);
}

#[test]
fn weekly_table() {
    let p = 7 * D;
    let rows: &[(i64, i64, i32)] = &[
        (6 * D, 80, 94),
        (6 * D, 43, 51),
        (6 * D, 42, 50),
        (6 * D, 18, 2),
        (6 * D, 17, 0),
        (3 * D, 25, 59),
        (2 * D, 20, 50),
        (2 * D, 5, 0),
        (20 * H, 31, 0),
        (20 * H, 30, 4),
        (20 * H, 20, 36),
        (20 * H, 15, 52),
        (20 * H, 10, 68),
        (20 * H, 0, 100),
        (20 * H, 40, -29),
    ];
    for &(secs, left, want) in rows {
        assert_eq!(
            score(left, Some(secs), p, &weekly()),
            want,
            "{secs}s/{left}"
        );
    }
}

#[test]
fn five_hour_table() {
    let p = 5 * H;
    let rows: &[(i64, i64, i32)] = &[
        (4 * H, 70, 87),
        (4 * H, 30, 29),
        (4 * H, 10, 0),
        (2 * H, 25, 62),
        (1800, 50, -61),
        (1800, 20, 36),
        (1800, 10, 68),
    ];
    for &(secs, left, want) in rows {
        assert_eq!(score(left, Some(secs), p, &five()), want, "{secs}s/{left}");
    }
}

#[test]
fn final_boundary_is_inclusive() {
    let p = 5 * H;
    // secs == final: final stretch line; final + 1: early line
    assert_eq!(score(15, Some(H), p, &five()), 52);
    assert_eq!(score(15, Some(H + 1), p, &five()), 33);
}

#[test]
fn share_below_warn_still_rises_to_full() {
    // 30 min of a week left outside the final stretch is impossible; use a
    // custom rule whose final stretch is 0 so share (7%) < warn (20%)
    let mut r = weekly();
    r.final_secs = 0;
    let p = 7 * D;
    let secs = 7 * D / 14; // share 7
    assert_eq!(score(21, Some(secs), p, &r), 100);
    assert_eq!(score(20, Some(secs), p, &r), 50);
    assert_eq!(score(5, Some(secs), p, &r), 0);
}

#[test]
fn no_secs_uses_floors_alone() {
    let p = 7 * D;
    assert_eq!(score(5, None, p, &weekly()), 0);
    assert_eq!(score(20, None, p, &weekly()), 50);
    assert_eq!(score(21, None, p, &weekly()), 100);
    assert_eq!(score(12, None, p, &weekly()), 23);
}

#[test]
fn pace_rule_uses_default_floors() {
    let mut r = weekly();
    r.early = Early::Pace;
    assert_eq!(
        score(6, Some(6 * D), 7 * D, &r),
        score(6, Some(6 * D), 7 * D, &weekly())
    );
}

#[test]
fn extreme_inputs_never_overflow() {
    let ints = [i64::MIN, -1, 0, 1, i64::MAX];
    for rule in [weekly(), five()] {
        for pct in ints {
            for secs in ints {
                for period in ints {
                    let _ = score(pct, Some(secs), period, &rule);
                }
            }
        }
    }
}

#[test]
fn extreme_metric_inputs_never_overflow() {
    use hop_bar::pace::metric_score;
    let ints = [i64::MIN, -1, 0, 1, i64::MAX];
    let rule = weekly();
    for remaining in ints {
        for granted in ints {
            for secs in ints {
                for period in ints {
                    let _ = metric_score(remaining, granted, Some(secs), period, &rule);
                }
            }
        }
    }
}
