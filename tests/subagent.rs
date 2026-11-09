//! `bar subagent`: subagentStatusLine input parsed, one `{"id","content"}`
//! line per row, the `subagent` layout, row placeholders and fitting.

mod common;

use assert_cmd::Command;
use hop_bar::conf::{self, LoadResult};
use hop_bar::render::{self, ColdRunner};
use hop_bar::session::Session;
use hop_bar::style::strip_codes;
use hop_bar::subagent::{self, model_name, Payload, Task};
use serde_json::Value;

/// 2026-01-01T00:00:00Z
const NOW: i64 = 1_767_225_600;

fn input() -> String {
    let start = (NOW - 95) * 1000;
    format!(
        r#"{{"session_id":"s1","transcript_path":"/x.jsonl","cwd":"/nonexistent/proj",
        "hook_event_name":"SubagentStatusLine","columns":120,"tasks":[
        {{"id":"a1","type":"local_agent","agentType":"Explore","status":"running",
          "description":"Find the loader","label":"Reading src/conf.rs","startTime":{start},
          "model":"claude-opus-5-5","effort":"high","contextWindowSize":200000,
          "tokenCount":41300,"tokenSamples":[1000,20000,41300],"cwd":"/nonexistent/wt"}},
        {{"id":"a2","name":"reviewer","type":"local_agent","agentType":"code-reviewer",
          "status":"completed","description":"Review","label":"Review","startTime":1,
          "tokenCount":900,"tokenSamples":[]}},
        {{"type":"local_agent","status":"running","description":"no id"}},
        {{"id":"a3","type":"local_agent","status":"failed","description":"Broke",
          "label":"Broke","startTime":1,"effort":32000,"tokenCount":5}}
        ]}}"#
    )
}

fn cfg(text: &str) -> conf::Config {
    conf::parse(text).unwrap()
}

fn rows(p: &Payload, c: &conf::Config) -> Vec<(String, String)> {
    subagent::rows(p, c, NOW, &ColdRunner, None)
}

fn plain(rows: &[(String, String)]) -> Vec<(String, String)> {
    rows.iter()
        .map(|(id, c)| (id.clone(), strip_codes(c)))
        .collect()
}

#[test]
fn parses_the_documented_task_fields() {
    let p = subagent::parse(&input());
    assert_eq!(p.session_id, "s1");
    assert_eq!(p.cwd, "/nonexistent/proj");
    assert_eq!(p.columns, Some(120));
    let ids: Vec<&str> = p.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["a1", "a2", "a3"], "a task without an id is dropped");
    let t = &p.tasks[0];
    assert_eq!(t.kind, "local_agent");
    assert_eq!(t.agent_type, "Explore");
    assert_eq!(t.label, "Reading src/conf.rs");
    assert_eq!(t.model, "claude-opus-5-5");
    assert_eq!(t.effort, "high");
    assert_eq!(t.context_size, Some(200_000));
    assert_eq!(t.tokens, Some(41_300));
    assert_eq!(t.samples, [1000, 20000, 41300]);
    assert_eq!(t.cwd, "/nonexistent/wt");
    assert_eq!(p.tasks[1].name, "reviewer");
    assert_eq!(
        p.tasks[2].effort, "32000",
        "a numeric effort is a token budget"
    );
}

#[test]
fn malformed_input_has_no_tasks() {
    for bad in [
        "",
        "null",
        "[]",
        "{\"tasks\":5}",
        "{\"tasks\":[1,\"x\",{}]}",
    ] {
        assert!(subagent::parse(bad).tasks.is_empty(), "{bad}");
    }
    let many: Vec<String> = (0..200).map(|i| format!("{{\"id\":\"t{i}\"}}")).collect();
    let p = subagent::parse(&format!("{{\"tasks\":[{}]}}", many.join(",")));
    assert_eq!(p.tasks.len(), subagent::MAX_TASKS);
    let samples: Vec<String> = (0..40).map(|i| i.to_string()).collect();
    let p = subagent::parse(&format!(
        "{{\"tasks\":[{{\"id\":\"t\",\"tokenSamples\":[{}]}}]}}",
        samples.join(",")
    ));
    assert_eq!(p.tasks[0].samples.len(), subagent::MAX_SAMPLES);
    assert_eq!(p.tasks[0].samples.last(), Some(&39), "the newest are kept");
}

#[test]
fn model_ids_read_as_names() {
    assert_eq!(model_name("claude-opus-5-5"), "Opus 5.5");
    assert_eq!(model_name("claude-sonnet-5"), "Sonnet 5");
    assert_eq!(model_name("claude-haiku-4-5-20251001"), "Haiku 4.5");
    assert_eq!(model_name("claude-opus-4-6[1m]"), "Opus 4.6");
    assert_eq!(model_name("gpt-x"), "gpt-x");
    assert_eq!(model_name("claude-"), "claude-");
}

#[test]
fn default_layout_draws_each_row() {
    let p = subagent::parse(&input());
    let r = plain(&rows(&p, &cfg("")));
    assert_eq!(
        r,
        [
            (
                "a1".to_string(),
                "● Explore │ Reading src/conf.rs │ Opus 5.5[high] │ 41.3k │ 1m".to_string()
            ),
            ("a2".into(), "✓ reviewer │ Review │ 900".into()),
            ("a3".into(), "✗ │ Broke │ [32000] │ 5".into()),
        ]
    );
}

#[test]
fn row_placeholders_and_session_ones() {
    let p = subagent::parse(&input());
    let c = cfg(concat!(
        "set subagent \"#{task.status[show=both]} #{task.type} #{task.desc[max=6]} ",
        "#{task.trend} #{ctx} #{cwd[base=1]}\"\n"
    ));
    let r = plain(&rows(&p, &c));
    assert_eq!(r[0].1, "● running Explore Find … ▁▂▂ ctx 21% wt");
    assert_eq!(r[1].1, "✓ completed code-reviewer Review proj");
}

#[test]
fn trend_scales_to_the_window_and_takes_width() {
    let p = subagent::parse(&input());
    let r = plain(&rows(&p, &cfg("set subagent \"#{task.trend[width=2]}\"")));
    assert_eq!(r.len(), 1, "a row with fewer than 2 samples draws nothing");
    assert_eq!(r[0].1.chars().count(), 2);
}

#[test]
fn elapsed_only_while_running() {
    let p = subagent::parse(&input());
    let r = plain(&rows(&p, &cfg("set subagent \"#{task.elapsed}\"")));
    assert_eq!(r, [("a1".to_string(), "1m".to_string())]);
}

#[test]
fn empty_layout_and_empty_rows_keep_claude_codes_rows() {
    let p = subagent::parse(&input());
    assert!(rows(&p, &cfg("set subagent \"\"")).is_empty());
    let r = rows(&p, &cfg("set subagent \"#{task.name}\""));
    let ids: Vec<&str> = r.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["a1", "a2"], "a3 has no name or type");
}

#[test]
fn trailing_segments_drop_to_fit_columns() {
    let mut p = subagent::parse(&input());
    p.columns = Some(31);
    let r = plain(&rows(&p, &cfg("")));
    assert_eq!(r[0].1, "● Explore │ Reading src/conf.rs");
    p.columns = Some(3);
    let r = plain(&rows(&p, &cfg("")));
    assert_eq!(r[0].1, "● Explore", "the first segment is always kept");
}

#[test]
fn plugins_do_not_render_in_rows_and_rows_not_in_the_status_line() {
    let p = subagent::parse(&input());
    let r = plain(&rows(
        &p,
        &cfg("set subagent \"#{nosuchplugin} #{task.name}\""),
    ));
    assert_eq!(r[0].1, "Explore");
    let line = render::render_in(
        &Session::default(),
        &cfg("set left \"#{task.name}x\""),
        None,
        &ColdRunner,
        None,
        &[],
    );
    assert_eq!(line, "");
}

#[test]
fn effort_is_the_tasks_own() {
    // no effort sent: nothing, never the user's settings
    let p = Payload {
        tasks: vec![Task {
            id: "t".into(),
            model: "claude-opus-5-5".into(),
            ..Task::default()
        }],
        ..Payload::default()
    };
    let r = plain(&rows(&p, &cfg("set subagent \"#{model}#{effort}\"")));
    assert_eq!(r[0].1, "Opus 5.5");
}

#[test]
fn skins_paint_rows() {
    let p = subagent::parse(&input());
    let r = rows(&p, &cfg("set skin powerline\nset palette nord\n"));
    assert!(r[0].1.contains("\x1b[48;5;"), "{:?}", r[0].1);
    assert!(!r[0].1.contains('\n'));
}

#[test]
fn run_writes_json_lines_and_nothing_on_config_error() {
    let out = subagent::run(&input(), |_| LoadResult::Missing, NOW, &ColdRunner, None);
    let lines: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["id"], "a1");
    assert!(lines[0]["content"].as_str().unwrap().contains("Explore"));
    let bad = conf::LoadResult::Invalid(Vec::new());
    assert_eq!(subagent::run(&input(), |_| bad, NOW, &ColdRunner, None), "");
    let none = subagent::run("{}", |_| unreachable!(), NOW, &ColdRunner, None);
    assert_eq!(none, "", "no tasks: no config read");
}

fn isolated(args: &[&str]) -> (Command, std::path::PathBuf) {
    let home = std::env::temp_dir().join(format!(
        "bar-subagent-{}-{}",
        std::process::id(),
        args.join("-").replace('/', "_")
    ));
    let (iso, cwd) = common::isolation(&home);
    let mut c = Command::cargo_bin("bar").unwrap();
    c.args(args)
        .envs(iso)
        .current_dir(cwd)
        .env("COLUMNS", "200");
    (c, home)
}

#[test]
fn cli_subagent_prints_one_line_per_row() {
    let (mut c, home) = isolated(&["subagent"]);
    let out = c
        .write_stdin(input())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let ids: Vec<String> = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["id"].to_string())
        .collect();
    assert_eq!(ids, ["\"a1\"", "\"a2\"", "\"a3\""]);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn cli_subagent_with_bad_input_prints_nothing() {
    for (i, bad) in ["", "not json", "{\"tasks\":[]}"].iter().enumerate() {
        let (mut c, home) = isolated(&["subagent", &i.to_string()][..1]);
        c.write_stdin(*bad).assert().success().stdout("");
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[test]
fn cli_preview_subagent_draws_the_sample_rows() {
    let (mut c, home) = isolated(&["preview", "--subagent"]);
    let out = c.assert().success().get_output().stdout.clone();
    let text = strip_codes(&String::from_utf8(out).unwrap());
    let rows: Vec<&str> = text.lines().collect();
    assert_eq!(rows.len(), 3, "{text}");
    assert!(
        rows[0].starts_with("● Explore │ Reading src/conf.rs │ Sonnet 5"),
        "{text}"
    );
    assert!(
        rows[1].contains("reviewer") && rows[1].contains("Opus 5.5[high]"),
        "{text}"
    );
    assert!(rows[2].starts_with("✓ general-purpose"), "{text}");
    let _ = std::fs::remove_dir_all(&home);
}
