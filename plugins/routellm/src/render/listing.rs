use super::item::caps_text;
use super::*;

/// `status`: state, aliases, tiers.
pub fn status_lines(s: &Snapshot, inst: &Instance, now: i64) -> Vec<String> {
    let c = &s.cache;
    let mut out = Vec::new();
    out.push(match (&s.status, c.problem) {
        (Status::AuthStopped(st), _) => {
            format!(
                "state: unknown (management refused the key, {st}; no calls until the key changes)"
            )
        }
        (Status::NoKey, _) => "state: unknown (no key)".into(),
        (Status::Off, _) => "state: unknown (management API off)".into(),
        (_, Some(Problem::Unconfigured)) => {
            format!(
                "state: not configured (no `{}` provider entry; run `bar rtellm setup`)",
                inst.provider
            )
        }
        (_, Some(Problem::Mismatch)) => format!(
            "state: refused (the `{}` entry does not point at {})",
            inst.provider, inst.routellm
        ),
        _ => {
            let w = state_word(s);
            let age = if c.at > 0 {
                format!(" ({}s ago)", (now - c.at).max(0))
            } else {
                String::new()
            };
            format!(
                "state: {}{age}",
                if w == "down" {
                    "on, RouteLLM not answering"
                } else {
                    w
                }
            )
        }
    });
    if let Some(e) = &c.entry {
        out.push(format!(
            "provider: {} (priority {})",
            inst.provider, e.priority
        ));
        for a in &e.aliases {
            let off = c
                .rows
                .iter()
                .find(|r| r.alias == a.alias)
                .map_or("no row".to_string(), |r| r.model.clone());
            out.push(format!("alias: {} -> tier {}; off: {off}", a.alias, a.tier));
        }
        if !armed(c) {
            out.push("warning: some aliases have no claude row, so OFF fails for them; run `bar rtellm setup`".into());
        }
    }
    match c.up {
        Some(true) => out.push(format!("routellm: up ({})", inst.routellm)),
        Some(false) => out.push(format!("routellm: not answering ({})", inst.routellm)),
        None => {}
    }
    if let Some(l) = &c.last {
        out.push(format!("last change: {} -> {} at {}", l.from, l.to, l.at));
    }
    out
}

/// `tiers`: one line per tier, aliased ones first.
pub fn tier_lines(s: &Snapshot, inst: &Instance, all: bool) -> Vec<String> {
    let c = &s.cache;
    let mut out = Vec::new();
    for t in &c.tiers {
        let alias = c
            .entry
            .as_ref()
            .and_then(|e| e.aliases.iter().find(|a| a.tier == t.id))
            .map(|a| a.alias.clone());
        if alias.is_none() && (!all || t.kind != "tier") {
            continue;
        }
        let mark = if inst.local_tiers.iter().any(|l| l == &t.id) {
            "local"
        } else {
            "may leave this host"
        };
        out.push(format!(
            "{} {} {} [{}]",
            t.id,
            alias.as_deref().unwrap_or("-"),
            caps_text(t),
            mark
        ));
    }
    if out.is_empty() {
        out.push("no tiers known; run `bar rtellm status --probe` while routing is on".into());
    }
    out
}
