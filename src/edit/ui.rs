//! Drawing: title, live preview, layout rows, the selected segment's
//! placeholders and options, pop-ups and the key help.

use super::ansi;
use super::app::{App, Choice, Mode, STATES};
use super::group;
use super::model::{row_key, Editor};
use super::schema::Kind;
use crate::plugin::ExtInfo;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use std::borrow::Cow;

const SEL: Style = Style::new().add_modifier(Modifier::REVERSED);
const MUTED: Style = Style::new().fg(Color::DarkGray);

/// A segment shown compactly: placeholder names (`…` when it has options)
/// with the text between them.
fn chip(seg: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    for ph in group::placeholders(seg) {
        if at > 0 && at == ph.span.start {
            out.push('+');
        }
        out.push_str(&seg[at..ph.span.start]);
        out.push_str(&ph.name);
        if !ph.options.is_empty() {
            out.push('…');
        }
        at = ph.span.end;
    }
    out.push_str(&seg[at..]);
    out
}

/// The editor as it would be with the highlighted choice applied, so the
/// preview follows the chooser.
fn previewed(app: &App) -> Cow<'_, Editor> {
    match app.mode {
        Mode::Choose { what, sel } => {
            let mut ed = app.ed.clone();
            what.apply(&mut ed, what.values()[sel]);
            Cow::Owned(ed)
        }
        _ => Cow::Borrowed(&app.ed),
    }
}

fn preview_lines(app: &App, ed: &Editor, width: u16, plugins: &[ExtInfo]) -> Vec<Line<'static>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    match ed.config() {
        Ok(c) => {
            let out = crate::preview::render_config(
                STATES[app.state],
                &c,
                Some(width as usize),
                now,
                Some(plugins),
            );
            ansi::lines(&out)
        }
        Err(errs) => errs
            .iter()
            .map(|e| {
                Line::styled(
                    format!("line {}: {}", e.line, e.message),
                    Style::new().fg(Color::Red),
                )
            })
            .collect(),
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let plugins = match previewed(app).config() {
        Ok(c) => app.plugin_infos(&c),
        Err(_) => Vec::new(),
    };
    let shown = previewed(app);
    let preview = preview_lines(app, &shown, area.width.saturating_sub(2), &plugins);
    let [title, prev, main, status, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(preview.len().clamp(1, 4) as u16 + 2),
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(title_line(app)), title);
    let block = Block::new().borders(Borders::ALL).title(format!(
        " preview: {} · {} ",
        STATES[app.state],
        style_summary(&shown)
    ));
    f.render_widget(Paragraph::new(preview).block(block), prev);
    let [rows, side] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(main);
    draw_rows(f, &app.ed, rows);
    draw_side(f, app, side);
    f.render_widget(Paragraph::new(app.msg.clone()), status);
    let help_text = Paragraph::new(Line::styled(help_text(&app.mode), MUTED))
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(help_text, help);
    match app.mode.clone() {
        Mode::Picker { query, sel } => draw_picker(f, app, &query, sel),
        Mode::Choose { what, sel } => draw_choose(f, what, sel),
        Mode::ConfirmQuit => draw_confirm(f),
        _ => {}
    }
}

fn title_line(app: &App) -> Line<'static> {
    let ed = &app.ed;
    let mut spans = vec![
        Span::styled(" bar edit ", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(ed.path.display().to_string()),
    ];
    if ed.dirty() {
        spans.push(Span::styled(" [modified]", Style::new().fg(Color::Yellow)));
    } else if !ed.exists {
        spans.push(Span::styled(" [new]", MUTED));
    }
    Line::from(spans)
}

/// `preset full · skin plain · palette default · icons none`.
fn style_summary(ed: &Editor) -> String {
    [Choice::Preset, Choice::Skin, Choice::Palette, Choice::Icons]
        .map(|c| {
            let v = c.current(ed).unwrap_or_else(|| "custom".into());
            format!("{} {v}", c.title())
        })
        .join(" · ")
}

fn draw_rows(f: &mut Frame, ed: &Editor, area: Rect) {
    let mut lines = Vec::new();
    for row in 0..ed.rows_shown() {
        let mut spans = vec![Span::styled(format!("{:<9}", row_key(row)), MUTED)];
        let segs = ed.segments(row);
        if segs.is_empty() {
            let st = if ed.cur.row == row { SEL } else { MUTED };
            spans.push(Span::styled(" (empty) ", st));
        }
        for (i, seg) in segs.iter().enumerate() {
            let st = if ed.cur.row == row && ed.cur.seg == i {
                SEL
            } else {
                Style::new()
            };
            spans.push(Span::styled(format!(" {} ", chip(seg)), st));
            if i + 1 < segs.len() {
                spans.push(Span::styled("│", MUTED));
            }
        }
        lines.push(Line::from(spans));
    }
    let block = Block::new().borders(Borders::ALL).title(" layout ");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_side(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::new().borders(Borders::ALL).title(" segment ");
    let Some(seg) = app.ed.selected_segment() else {
        let p = Paragraph::new(Line::styled("a adds a placeholder", MUTED)).block(block);
        f.render_widget(p, area);
        return;
    };
    let mut lines = vec![Line::raw(seg.clone()), Line::raw("")];
    for (i, ph) in group::placeholders(&seg).iter().enumerate() {
        let st = if i == app.ed.cur.ph {
            SEL
        } else {
            Style::new()
        };
        lines.push(Line::styled(format!("#{{{}}}", ph.name), st));
    }
    let (sel, input) = match &app.mode {
        Mode::Options { sel } => (Some(*sel), None),
        Mode::Input { sel, key, buf } => (Some(*sel), Some((key.clone(), buf.clone()))),
        _ => (None, None),
    };
    if let (Some(sel), Some(ph)) = (sel, app.ed.selected_placeholder()) {
        lines.push(Line::raw(""));
        for (i, o) in app.option_rows().iter().enumerate() {
            let value = match &input {
                Some((k, buf)) if *k == o.key => format!("{buf}▏"),
                _ => ph
                    .options
                    .get(&o.key)
                    .map(str::to_string)
                    .or_else(|| o.default.as_ref().map(|d| format!("({d})")))
                    .unwrap_or_else(|| "-".into()),
            };
            let hint = match &o.kind {
                Kind::Choice(v) | Kind::List(v) => v.join("|"),
                Kind::Int { .. } => "number".into(),
                Kind::Text => "text".into(),
            };
            let st = if i == sel { SEL } else { Style::new() };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<9} {value}", o.key), st),
                Span::styled(format!("  {hint}"), MUTED),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// A box of `w` by `h` cells centred in the frame.
fn popup(f: &mut Frame, w: u16, h: u16, title: &str) -> Rect {
    let a = f.area();
    let (w, h) = (w.min(a.width), h.min(a.height));
    let r = Rect::new(a.x + (a.width - w) / 2, a.y + (a.height - h) / 2, w, h);
    f.render_widget(Clear, r);
    let block = Block::new().borders(Borders::ALL).title(title.to_string());
    let inner = block.inner(r);
    f.render_widget(block, r);
    inner
}

fn draw_picker(f: &mut Frame, app: &App, query: &str, sel: usize) {
    let inner = popup(f, 60, 18, " add placeholder ");
    let [q, list] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new(format!("> {query}▏")), q);
    let items: Vec<ListItem> = app
        .matches(query)
        .into_iter()
        .map(|(n, d)| {
            ListItem::new(Line::from(vec![
                Span::raw(format!("{n:<12}")),
                Span::styled(d, MUTED),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected(Some(sel));
    f.render_stateful_widget(List::new(items).highlight_style(SEL), list, &mut st);
}

fn draw_choose(f: &mut Frame, what: Choice, sel: usize) {
    let values = what.values();
    let inner = popup(
        f,
        30,
        values.len() as u16 + 2,
        &format!(" {} ", what.title()),
    );
    let items: Vec<ListItem> = values.iter().map(|v| ListItem::new(*v)).collect();
    let mut st = ListState::default().with_selected(Some(sel));
    f.render_stateful_widget(List::new(items).highlight_style(SEL), inner, &mut st);
}

fn draw_confirm(f: &mut Frame) {
    let inner = popup(f, 44, 3, " unsaved changes ");
    f.render_widget(
        Paragraph::new("save before quitting? y/n, esc stays"),
        inner,
    );
}

fn help_text(mode: &Mode) -> &'static str {
    match mode {
        Mode::Normal => "arrows move · H/L J/K move segment · a add · d delete · enter options · tab next · p preset · s skin · c palette · i icons · t state · u/U undo/redo · w save · q quit",
        Mode::Picker { .. } => "type to search · up/down · enter add · esc cancel",
        Mode::Options { .. } => "up/down · left/right cycle · enter type a value · backspace unset · esc back",
        Mode::Input { .. } => "enter set (empty unsets) · esc cancel",
        Mode::Choose { .. } => "up/down preview · enter apply · esc cancel",
        Mode::ConfirmQuit => "y save and quit · n quit without saving · esc back",
    }
}
