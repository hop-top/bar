use crate::edit::group::{
    join, placeholders, set_option, split, value_error, with_options, write_placeholder,
};
use crate::format::{parse, segments, Token};
use crate::options::Options;

fn opts(pairs: &[(&str, &str)]) -> Options {
    let mut o = Options::default();
    for (k, v) in pairs {
        o.set(k, v);
    }
    o
}

#[test]
fn split_matches_the_renderer() {
    for src in [
        "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}",
        " │ #{a} │ │ #{b} │ ",
        "#{issues[label=\"x │ y\"]} │ #{ctx}",
        "plain text │ #{usage[type=5h]}",
    ] {
        let mine = split(src, "│");
        let theirs = segments(&parse(src).unwrap(), "│");
        assert_eq!(mine.len(), theirs.len(), "{src}");
        for (m, t) in mine.iter().zip(&theirs) {
            let m: Vec<Token> = parse(m).unwrap();
            // the renderer trims text beside a split; so does split
            assert_eq!(&m, t, "{src}");
        }
    }
}

#[test]
fn split_and_join_round_trip() {
    let src = "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}";
    assert_eq!(join(&split(src, "│"), "│"), src);
    assert_eq!(split("#{a} | #{b}", "|"), ["#{a}", "#{b}"]);
}

#[test]
fn placeholders_with_spans() {
    let seg = "#{repo}:#{branch[max=20]}";
    let p = placeholders(seg);
    assert_eq!(p.len(), 2);
    assert_eq!(p[1].name, "branch");
    assert_eq!(&seg[p[1].span.clone()], "#{branch[max=20]}");
    assert_eq!(p[1].options.get("max"), Some("20"));
}

#[test]
fn write_placeholder_forms() {
    assert_eq!(write_placeholder("ctx", &Options::default()), "#{ctx}");
    assert_eq!(
        write_placeholder("usage", &opts(&[("type", "5h"), ("show", "time,pct")])),
        "#{usage[type=5h,show=time,pct]}"
    );
    // a bare key after a list option would join the list
    let w = write_placeholder("usage", &opts(&[("show", "bar,pct"), ("x", "")]));
    assert_eq!(w, "#{usage[show=bar,pct,x=]}");
    match &parse(&w).unwrap()[0] {
        Token::Placeholder { options, .. } => {
            assert_eq!(options.get("show"), Some("bar,pct"));
            assert_eq!(options.get("x"), Some(""));
        }
        t => panic!("{t:?}"),
    }
}

#[test]
fn with_options_touches_only_that_placeholder() {
    let seg = "#{repo}:#{branch}";
    let ph = &placeholders(seg)[1];
    assert_eq!(
        with_options(seg, ph, &opts(&[("max", "9")])),
        "#{repo}:#{branch[max=9]}"
    );
}

#[test]
fn set_option_replaces_in_place_or_removes() {
    let o = opts(&[("type", "5h"), ("width", "5")]);
    let r = set_option(&o, "type", Some("7d"));
    assert_eq!(r, opts(&[("type", "7d"), ("width", "5")]));
    assert_eq!(set_option(&o, "type", None), opts(&[("width", "5")]));
    assert_eq!(
        set_option(&o, "color", Some("gradient")),
        opts(&[("type", "5h"), ("width", "5"), ("color", "gradient")])
    );
}

#[test]
fn value_error_refuses_what_cannot_be_written() {
    assert!(value_error("label", "a\"b").is_some());
    assert!(value_error("label", "a,b").is_some());
    assert!(value_error("max", "1}").is_some());
    assert!(value_error("show", "time,pct").is_none());
    assert!(value_error("show", "a]").is_some());
    assert!(value_error("", "x").is_some());
    assert!(value_error("author", "@me").is_none());
}
