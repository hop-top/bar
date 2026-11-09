use crate::conf::tokenize;
use crate::edit::doc::{quote, tokens, Doc};

#[test]
fn tokens_match_the_config_tokenizer() {
    for line in [
        "set left \"#{model} │ #{ctx}\"",
        "  set   skin powerline   # trailing",
        "set left \"a\"b\"c d\"",
        "default usage show=time,bar",
        "# only a comment",
        "",
        "set ramp \"196 202 208 214 220 226 190 154 118 82 46\"\r\n",
    ] {
        let mine: Vec<String> = tokens(line).unwrap().into_iter().map(|t| t.value).collect();
        assert_eq!(mine, tokenize(line).unwrap(), "{line:?}");
    }
    assert!(tokens("set left \"open").is_none());
}

#[test]
fn token_spans_cover_the_raw_text() {
    let line = "set skin  \"power line\" # c";
    let t = tokens(line).unwrap();
    assert_eq!(&line[t[1].span.clone()], "skin");
    assert_eq!(&line[t[2].span.clone()], "\"power line\"");
    assert!(t[2].quoted && !t[1].quoted);
}

#[test]
fn quote_bare_only_for_plain_words() {
    assert_eq!(quote("nord", true), "nord");
    assert_eq!(quote("nord", false), "\"nord\"");
    assert_eq!(quote("a b", true), "\"a b\"");
    assert_eq!(quote("a#b", true), "\"a#b\"");
    assert_eq!(quote("", true), "\"\"");
}

const FILE: &str = "# my bar\r\nset skin plain   # was powerline\r\n\r\nset left \"#{model}\"\r\nunknown stuff here\r\nset skin capsule\r\n";

#[test]
fn set_rewrites_only_the_winning_value() {
    let mut d = Doc::new(FILE);
    assert!(d.set(&|k| k == "skin", "skin", "powerline"));
    assert_eq!(
        d.text(),
        FILE.replace("set skin capsule", "set skin powerline")
    );
    // quoted stays quoted, comment and CRLF kept
    let mut d = Doc::new(FILE);
    d.set(&|k| k == "left", "left", "#{model} │ #{ctx}");
    assert_eq!(
        d.text(),
        FILE.replace("\"#{model}\"", "\"#{model} │ #{ctx}\"")
    );
}

#[test]
fn set_keeps_trailing_comment() {
    let mut d = Doc::new("set skin plain   # was powerline\n");
    d.set(&|k| k == "skin", "skin", "capsule");
    assert_eq!(d.text(), "set skin capsule   # was powerline\n");
}

#[test]
fn set_appends_with_the_files_line_ending() {
    let mut d = Doc::new("# c\r\nset skin plain");
    assert!(d.set(&|k| k == "palette", "palette", "nord"));
    assert_eq!(d.text(), "# c\r\nset skin plain\r\nset palette nord\r\n");
    let mut d = Doc::new("");
    d.set(&|k| k == "left", "left", "#{model}");
    assert_eq!(d.text(), "set left \"#{model}\"\n");
}

#[test]
fn set_same_value_is_no_change() {
    let mut d = Doc::new(FILE);
    assert!(!d.set(&|k| k == "skin", "skin", "capsule"));
    assert_eq!(d.text(), FILE);
}

#[test]
fn get_reads_the_last_line() {
    let d = Doc::new(FILE);
    assert_eq!(d.get(&|k| k == "skin").as_deref(), Some("capsule"));
    assert_eq!(d.get(&|k| k == "palette"), None);
}

#[test]
fn remove_drops_every_line_for_the_key() {
    let mut d = Doc::new(FILE);
    assert!(d.remove(&|k| k == "skin"));
    assert_eq!(
        d.text(),
        "# my bar\r\n\r\nset left \"#{model}\"\r\nunknown stuff here\r\n"
    );
    assert!(!d.remove(&|k| k == "skin"));
}

#[test]
fn untouched_doc_round_trips() {
    for t in [FILE, "", "no newline", "a\n\n\nb\n"] {
        assert_eq!(Doc::new(t).text(), t);
    }
}

#[test]
fn set_keeps_the_quoting_style() {
    let mut d = Doc::new("set skin \"plain\"\nset palette nord\n");
    d.set(&|k| k == "skin", "skin", "capsule");
    d.set(&|k| k == "palette", "palette", "gruvbox");
    assert_eq!(d.text(), "set skin \"capsule\"\nset palette gruvbox\n");
}
