mod common;
use hop_bar::conf::{self, Amount, Early, LoadResult, Wrap};
use std::fs;

const SPEC: &str = include_str!("fixtures/spec.conf");

#[test]
fn parses_spec_sample() {
    let c = conf::parse(SPEC).unwrap();
    assert_eq!(c.gap, 3);
    assert_eq!(c.wrap, Wrap::Off);
    assert_eq!(c.refresh, 5);
    assert!(c.left.starts_with("#{model}#{effort} │ #{ctx}"));
    assert_eq!(c.right, "#{account}");
    assert_eq!(c.colors.good, 114);
    assert_eq!(c.colors.warn, 179);
    assert_eq!(c.colors.bad, 203);
    assert_eq!(
        c.ramp,
        [196, 202, 208, 214, 220, 226, 190, 154, 118, 82, 46]
    );
    let globs: Vec<_> = c.accounts.iter().map(|a| a.glob.as_str()).collect();
    assert_eq!(globs, ["*acme*", "*@widgets.example.org", "*@example.net"]);
    assert_eq!(c.accounts[0].label, "acme");
    assert_eq!(c.accounts[0].color, 141);
    let r = &c.rules[0];
    assert_eq!(r.target, "usage[type=5h]");
    assert_eq!(
        r.early,
        Early::Thresholds {
            warn: Amount::Percent(20),
            bad: Amount::Percent(5)
        }
    );
    assert_eq!(r.final_secs, 3600);
    assert_eq!(r.final_bad, Amount::Percent(30));
    assert_eq!(c.rules[1].final_secs, 86400);
    assert_eq!(c.defaults["prs"].get("limit"), Some("5"));
    assert_eq!(c.defaults["usage"].get("click"), Some("toggle"));
    let p = &c.plugins["claude-credits"];
    assert_eq!(p.get("ttl"), Some("5m"));
    assert_eq!(p.get("keychain"), Some("claude-console-session-<label>"));
}

#[test]
fn parses_credits_rule() {
    let c = conf::parse("rule credits early pace final=3d bad=$50\n").unwrap();
    let r = &c.rules[0];
    assert_eq!(r.target, "credits");
    assert_eq!(r.early, Early::Pace);
    assert_eq!(r.final_secs, 259_200);
    assert_eq!(r.final_bad, Amount::Cents(5000));
}

#[test]
fn comments_and_blank_lines_ignored() {
    let c = conf::parse("# top\n\n   \nset gap 4 # trailing\nset left \"a # b\"\n").unwrap();
    assert_eq!(c.gap, 4);
    assert_eq!(c.left, "a # b");
}

#[test]
fn defaults_when_empty() {
    let c = conf::parse("").unwrap();
    assert_eq!(c.separator, "│");
    assert_eq!(c.gap, 3);
    assert_eq!(c.refresh, 5);
    assert_eq!(c.colors.good, 114);
    assert_eq!(c.ramp[0], 196);
    assert_eq!(c.left, "");
}

#[test]
fn unknown_directive_reports_line_number() {
    let e = conf::parse("set gap 3\n\nbogus x\n").unwrap_err();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].line, 3);
}

#[test]
fn unknown_set_key_and_bad_ramp_report_line() {
    let e = conf::parse("set nope 1\nset ramp \"1 2 3\"\nset color.good 300\n").unwrap_err();
    let lines: Vec<_> = e.iter().map(|x| x.line).collect();
    assert_eq!(lines, [1, 2, 3]);
}

#[test]
fn unterminated_quote_reports_line_number() {
    let e = conf::parse("set gap 3\nset left \"oops\n").unwrap_err();
    assert_eq!(e[0].line, 2);
}

#[test]
fn bad_rule_reports_line_number() {
    let e = conf::parse("\nrule usage early warn=1% final=1h bad=3%\n").unwrap_err();
    assert_eq!(e[0].line, 2);
}

#[test]
fn options_merge_over_defaults() {
    let c = conf::parse("default prs limit=5 status=open\n").unwrap();
    let mut o = hop_bar::options::Options::default();
    o.set("limit", "3");
    o.set("author", "@me");
    let m = o.merged_over(&c.defaults["prs"]);
    let pairs: Vec<_> = m.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(
        pairs,
        [("limit", "3"), ("status", "open"), ("author", "@me")]
    );
}

fn lookup<'a>(xdg: Option<&'a str>) -> impl Fn(&str) -> Option<String> + 'a {
    move |k| {
        (k == "XDG_CONFIG_HOME")
            .then(|| xdg.map(String::from))
            .flatten()
    }
}

fn write(dir: &std::path::Path, body: &str) {
    fs::create_dir_all(dir.join("bar")).unwrap();
    fs::write(dir.join("bar/bar.conf"), body).unwrap();
}

#[test]
fn config_search_order() {
    let t = common::tmp::Dir::new(&format!("bar-conf-{}", std::process::id()));
    let (xdg, home, kit) = (t.join("xdg"), t.join("home"), t.join("kit"));
    let dot = home.join(".config");
    fs::create_dir_all(&xdg).unwrap();

    let load = || conf::load_from(&lookup(xdg.to_str()), &home, &kit.join("bar"));
    assert!(matches!(load(), LoadResult::Missing));

    write(&kit, "set gap 9\n");
    assert!(matches!(load(), LoadResult::Ok(c) if c.gap == 9));
    write(&dot, "set gap 8\n");
    assert!(matches!(load(), LoadResult::Ok(c) if c.gap == 8));
    write(&xdg, "set gap 7\n");
    assert!(matches!(load(), LoadResult::Ok(c) if c.gap == 7));
    write(&xdg, "set gap x\n");
    assert!(matches!(load(), LoadResult::Invalid(e) if e[0].line == 1));
}

#[test]
fn missing_file_is_missing_not_error() {
    let t = common::tmp::Dir::new(&format!("bar-conf-missing-{}", std::process::id()));
    let r = conf::load_from(&lookup(None), &t, &t.join("kit/bar"));
    assert!(matches!(r, LoadResult::Missing));
}

#[test]
fn overflowing_duration_and_amount_report_line_number() {
    let src = "set refresh 99999999999999999d\nrule credits early pace final=1h bad=$999999999999999999\n";
    let e = conf::parse(src).unwrap_err();
    let lines: Vec<_> = e.iter().map(|x| x.line).collect();
    assert_eq!(lines, [1, 2]);
    assert!(e[0].message.contains("invalid duration"));
    assert!(e[1].message.contains("invalid amount"));
}

#[test]
fn unquoted_hex_colour_gets_a_hint() {
    const HINT: &str = "quote hex colours: \"#rrggbb\"";
    for line in [
        "set color.good #a3be8c",
        "set gradient #ff0000:#0000ff",
        "account \"*@work.com\" work #88c0d0",
        "set color.bad    #BF616A   ",
    ] {
        let e = conf::parse(line).unwrap_err();
        assert!(e[0].message.contains(HINT), "{line}: {e:?}");
        assert_eq!(e[0].line, 1);
    }
    // a comment that is no colour, or a quoted one, gets no hint
    for line in ["set color.good # 114 here", "set color.good #a3be"] {
        let e = conf::parse(line).unwrap_err();
        assert!(!e[0].message.contains(HINT), "{line}: {e:?}");
    }
    assert!(conf::parse("set color.good \"#a3be8c\" # #ffffff").is_ok());
}
