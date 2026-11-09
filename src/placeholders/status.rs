//! `status`: Claude service status from status.claude.com. Off unless
//! `set service-status on`; a background job fetches it every 5 minutes.

use super::{link_enabled, truncate, Ctx, Placeholder};
use crate::cache::Job;
use crate::options::Options;
use crate::status::{self, Impact, Snapshot};
use crate::style::{bold, dim, fg, link, sanitize};

pub struct Status;

/// Parts of the output, in the order given by option `show`.
const PARTS: [&str; 3] = ["state", "text", "strip"];

fn parts(opts: &Options) -> Vec<&str> {
    let list: Vec<&str> = opts
        .get("show")
        .unwrap_or("state")
        .split(',')
        .map(str::trim)
        .filter(|p| PARTS.contains(p))
        .collect();
    if list.is_empty() {
        vec!["state"]
    } else {
        list
    }
}

fn paint(ctx: &Ctx, i: Impact, text: &str) -> String {
    let c = &ctx.config.colors;
    match i {
        Impact::None => fg(c.good, text),
        Impact::Minor => fg(c.warn, text),
        Impact::Major => fg(c.bad, text),
        Impact::Critical => bold(&fg(c.bad, text)),
    }
}

/// The output for `snap`; `None` when there is nothing to show.
pub fn fmt(ctx: &Ctx, opts: &Options, snap: &Snapshot, label: bool) -> Option<String> {
    let filter = status::filter_terms(opts.get("components"));
    let ok = matches!(opts.get("ok"), Some("1" | "true" | "yes" | "on"));
    let level = snap.level(&filter);
    let mut out: Vec<String> = Vec::new();
    for p in parts(opts) {
        match p {
            "state" if level > Impact::None || ok => {
                let word = if level == Impact::None {
                    "✓"
                } else {
                    level.word()
                };
                let word = paint(ctx, level, word);
                out.push(if label {
                    format!("{} {word}", dim("status"))
                } else {
                    word
                });
            }
            "text" => {
                if let Some(i) = snap.active(&filter) {
                    let t = truncate(sanitize(&i.title), opts);
                    out.push(paint(ctx, i.impact, &t));
                }
            }
            "strip" => {
                let hours = opts
                    .get("hours")
                    .and_then(|h| h.trim().parse::<i64>().ok())
                    .filter(|h| *h >= 1)
                    .unwrap_or(6);
                let cells = snap.strip(&filter, ctx.now, hours);
                if ok || cells.iter().any(|c| *c > Impact::None) {
                    let s: String = cells
                        .iter()
                        .map(|c| match c {
                            Impact::None => dim("▁"),
                            c => paint(ctx, *c, "█"),
                        })
                        .collect();
                    out.push(s);
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return None;
    }
    let text = out.join(" ");
    Some(if link_enabled(opts) {
        link(status::URL, &text)
    } else {
        text
    })
}

impl Placeholder for Status {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        self.render_labeled(ctx, opts, true)
    }

    fn render_labeled(&self, ctx: &Ctx, opts: &Options, label: bool) -> Option<String> {
        if !ctx.config.service_status {
            return None;
        }
        let cache = ctx.cache?;
        let strip = parts(opts).contains(&"strip");
        let raw = cache.read_or_refresh(
            status::cache_key(strip),
            status::TTL,
            &Job::Status { strip },
        )?;
        fmt(ctx, opts, &Snapshot::parse(&raw)?, label)
    }
}
