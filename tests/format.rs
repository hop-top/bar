use hop_bar::conf::Config;
use hop_bar::expand::{CommandRunner, Expander};
use hop_bar::format::{parse, segments, Token};
use hop_bar::options::Options;
use std::cell::Cell;

fn ph(t: &Token) -> (&str, &Options) {
    match t {
        Token::Placeholder { name, options } => (name, options),
        other => panic!("not a placeholder: {other:?}"),
    }
}

struct Fake {
    out: Option<String>,
    calls: Cell<usize>,
}

impl CommandRunner for Fake {
    fn run(&self, _cmd: &str) -> Option<String> {
        self.calls.set(self.calls.get() + 1);
        self.out.clone()
    }
}

fn fake(out: Option<&str>) -> Fake {
    Fake {
        out: out.map(String::from),
        calls: Cell::new(0),
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn splits_text_and_placeholders() {
    let t = parse("a #{model}#{ctx} b").unwrap();
    assert_eq!(t.len(), 4);
    assert_eq!(t[0], Token::Text("a ".into()));
    assert_eq!(ph(&t[1]).0, "model");
    assert_eq!(ph(&t[2]).0, "ctx");
    assert_eq!(t[3], Token::Text(" b".into()));
    assert!(parse("#{open").is_err());
}

#[test]
fn parses_options() {
    let t = parse("#{prs[limit=3,author=@me]}").unwrap();
    let (name, o) = ph(&t[0]);
    assert_eq!(name, "prs");
    assert_eq!(o.get("limit"), Some("3"));
    assert_eq!(o.get("author"), Some("@me"));
}

#[test]
fn value_keeps_pipes_and_ampersands() {
    let t = parse(r#"#{issues[label=(a|b)&"c d",x=!y]}"#).unwrap();
    let (_, o) = ph(&t[0]);
    assert_eq!(o.get("label"), Some(r#"(a|b)&"c d""#));
    assert_eq!(o.get("x"), Some("!y"));
}

#[test]
fn comma_inside_quotes_is_not_a_separator() {
    let t = parse(r#"#{issues[label="a,b",limit=2]}"#).unwrap();
    let (_, o) = ph(&t[0]);
    assert_eq!(o.get("label"), Some(r#""a,b""#));
    assert_eq!(o.get("limit"), Some("2"));
}

#[test]
fn inline_overrides_default() {
    let mut c = Config::default();
    let mut d = Options::default();
    d.set("limit", "5");
    d.set("author", "@me");
    c.defaults.insert("prs".into(), d);
    let t = parse("#{prs[limit=3]}").unwrap();
    let (name, o) = ph(&t[0]);
    let m = o.merged_over(&c.defaults[name]);
    assert_eq!(m.get("limit"), Some("3"));
    assert_eq!(m.get("author"), Some("@me"));
}

#[test]
fn expands_env() {
    let env = |k: &str| (k == "USER").then(|| "jad".to_string());
    let r = fake(None);
    let e = Expander::with_env(&r, &env);
    assert_eq!(e.expand("hi %$USER%!"), "hi jad!");
}

#[test]
fn unset_env_is_empty() {
    let r = fake(None);
    let e = Expander::with_env(&r, &no_env);
    assert_eq!(e.expand("[%$NOPE%]"), "[]");
}

#[test]
fn expands_command() {
    let r = fake(Some("octocat\n"));
    let e = Expander::with_env(&r, &no_env);
    assert_eq!(e.expand("%gh api user -q .login%"), "octocat");
    assert_eq!(e.expand("x%gh api user -q .login%"), "xoctocat");
    assert_eq!(r.calls.get(), 1);
}

#[test]
fn at_me_passes_through_unchanged() {
    let r = fake(Some("x"));
    let e = Expander::with_env(&r, &no_env);
    assert_eq!(e.expand("@me"), "@me");
    assert_eq!(e.expand("50% done"), "50% done");
    assert_eq!(r.calls.get(), 0);
}

#[test]
fn failing_command_expands_empty() {
    let r = fake(None);
    let e = Expander::with_env(&r, &no_env);
    assert_eq!(e.expand("a%false%b"), "ab");
}

#[test]
fn literal_separator_splits_segments() {
    let t = parse("#{model}#{effort} │ #{ctx} │ #{prs}").unwrap();
    let s = segments(&t, "│");
    assert_eq!(s.len(), 3);
    assert_eq!(s[0].len(), 2);
    assert_eq!(ph(&s[1][0]).0, "ctx");
    assert_eq!(ph(&s[2][0]).0, "prs");
}

fn seg(input: &str, sep: &str) -> Vec<Vec<Token>> {
    segments(&parse(input).unwrap(), sep)
}

#[test]
fn text_beside_separator_is_kept_trimmed() {
    let s = seg("foo │ #{x}", "│");
    assert_eq!(s.len(), 2);
    assert_eq!(s[0], vec![Token::Text("foo".into())]);
    assert_eq!(ph(&s[1][0]).0, "x");
}

#[test]
fn empty_segments_are_dropped() {
    for input in ["│ #{a}", "#{a} │", "#{a} │ │ #{b}", " │ "] {
        let s = seg(input, "│");
        assert!(s.iter().all(|v| !v.is_empty()), "{input}: {s:?}");
    }
    assert_eq!(seg("│ #{a}", "│").len(), 1);
    assert_eq!(seg("#{a} │", "│").len(), 1);
    assert_eq!(seg("#{a} │ │ #{b}", "│").len(), 2);
}

#[test]
fn configured_separator_is_trimmed() {
    let s = seg("#{a}│#{b}", " │ ");
    assert_eq!(s.len(), 2);
}

#[test]
fn bare_part_continues_previous_list_value() {
    let t = parse("#{usage[type=5h,show=bar,pct,width=3]}").unwrap();
    let (_, o) = ph(&t[0]);
    assert_eq!(o.get("show"), Some("bar,pct"));
    assert_eq!(o.get("type"), Some("5h"));
    assert_eq!(o.get("width"), Some("3"));
    assert_eq!(o.get("pct"), None);
}

#[test]
fn bare_flag_after_keyed_option_stays_a_flag() {
    let t = parse("#{x[limit=3,verbose]}").unwrap();
    let (_, o) = ph(&t[0]);
    assert_eq!(o.get("limit"), Some("3"));
    assert_eq!(o.get("verbose"), Some(""));
}

#[test]
fn list_continuation_follows_the_last_key_set() {
    let t = parse("#{usage[show=bar,a=1,show=pct,time]}").unwrap();
    let (_, o) = ph(&t[0]);
    assert_eq!(o.get("show"), Some("pct,time"));
    assert_eq!(o.get("a"), Some("1"));
    let t = parse("#{usage[alt=pct,time,show=bar,pct]}").unwrap();
    let (_, o) = ph(&t[0]);
    assert_eq!(o.get("alt"), Some("pct,time"));
    assert_eq!(o.get("show"), Some("bar,pct"));
    let t = parse("#{prs[checks=fail,pending,limit=3]}").unwrap();
    let (_, o) = ph(&t[0]);
    assert_eq!(o.get("checks"), Some("fail,pending"));
    assert_eq!(o.get("limit"), Some("3"));
}
