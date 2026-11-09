//! `set subagent`: what `bar check` says about it and what a project
//! config may set.

use hop_bar::check::{self, Level};
use hop_bar::conf::{layers, ProjectMode};

fn errors(text: &str) -> Vec<(usize, String)> {
    check::check_text(text, None, &[])
        .issues
        .into_iter()
        .filter(|i| i.level == Level::Error)
        .map(|i| (i.line, i.message))
        .collect()
}

#[test]
fn check_accepts_row_and_core_placeholders_in_subagent() {
    let text = concat!(
        "set left \"#{model}\"\n",
        "set subagent \"#{task.status[show=both]} #{task.name[max=20]} │ #{task.trend[width=4]} │ #{model}#{effort} │ #{ctx[show=tokens]}\"\n",
        "default task.label max=40\n",
    );
    assert_eq!(errors(text), []);
}

#[test]
fn check_rejects_row_placeholders_outside_subagent() {
    let e = errors("set left \"#{task.name}\"\n");
    assert_eq!(
        e,
        [(1, "`task.name` works only in `set subagent`".to_string())]
    );
}

#[test]
fn check_rejects_plugins_and_bad_options_in_subagent() {
    let e = errors("set subagent \"#{nosuch} #{task.status[show=x]} #{task.tokens[max=1]}\"\n");
    assert_eq!(
        e,
        [
            (
                1,
                "unknown placeholder `nosuch` (`set subagent` draws no plugins)".to_string()
            ),
            (1, "invalid value `x` for `show`".to_string()),
            (1, "unknown option `max` for `task.tokens`".to_string()),
        ]
    );
}

#[test]
fn project_subagent_layout_is_safe_without_commands() {
    let l = layers(Some("set left \"#{model}\"\n"), |m| {
        assert_eq!(m, ProjectMode::Safe);
        Some(Ok(
            "set subagent \"#{task.name}\"\nset subagent \"#{task.name[max=%cmd%]}\"\n".into(),
        ))
    });
    let c = l.config.unwrap();
    assert_eq!(c.subagent.as_deref(), Some("#{task.name}"));
    assert_eq!(l.dropped.len(), 1);
    assert_eq!(l.dropped[0].line, 2);
}
