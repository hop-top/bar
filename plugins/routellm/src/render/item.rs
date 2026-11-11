use super::*;

fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

fn expand(tpl: &str, inst: &Instance, alias: &str, tier: &str) -> String {
    tpl.replace("{instance}", &escape(&instance_id(&inst.origin)))
        .replace("{alias}", &escape(alias))
        .replace("{tier}", &escape(tier))
}

/// The segment's mark: nf-fa-random (U+F074), drawn dim in place of the
/// label while icons are on.
pub const ICON: &str = "\u{f074}";

/// The label drawn in place of the icon with `set icons none`.
pub const LABEL: &str = "RL";

/// The segment, one item: `alias[tier]` while routing is on, `tier[model]`
/// (the model the alias maps to) while it is off, `tier[\u{2717}]` when the
/// alias has no row and routing is off. The text carries the state colour
/// (`good` on, `dim` off, `bad` when RouteLLM does not answer or the alias
/// is refused). A session not on a `route-*` alias of the provider gets the
/// state word alone. `show` adds the separate `state`, `marker` and `caps`
/// items. The first item carries the mark; every item carries the toggle's
/// tokens, an item with a `link` keeps it. Hidden when the instance is not
/// drivable.
pub fn output(s: &Snapshot, inst: &Instance, now: i64, o: &Opts, model_id: &str) -> Output {
    if s.cache.problem.is_some() || (s.status == Status::Off) {
        return Output::hidden();
    }
    let session_alias = alias_of_model(model_id);
    if o.routed && session_alias.is_none() {
        return Output::hidden();
    }
    let w = state_word(s);
    let old = stale(&s.cache, now) && matches!(w, "on" | "off" | "down");
    if old && o.stale == Stale::Hide {
        return Output::hidden();
    }
    let dim = old && o.stale == Stale::Dim;
    let mark = if old && o.stale == Stale::Mark {
        " ~"
    } else {
        ""
    };
    let tokens: BTreeMap<String, String> = BTreeMap::from([
        ("instance".into(), instance_id(&inst.origin)),
        // what a click would do: the opposite of the current state
        ("to".into(), if w == "off" { "on" } else { "off" }.into()),
    ]);
    let tokened = |mut it: Output| {
        it.tokens = tokens.clone();
        it
    };
    let finish = |mut items: Vec<Output>| {
        if let Some(first) = items.first_mut() {
            first.label = Some(LABEL.into());
            first.icon = Some(ICON.into());
        }
        Output {
            items,
            tokens: tokens.clone(),
            ..Default::default()
        }
    };
    // no state to speak of: the words stay, without a mark
    if matches!(w, "key?" | "?") {
        let (text, style) = if w == "key?" {
            ("RL key?", "bad")
        } else {
            ("RL ?", "dim")
        };
        return Output {
            items: vec![tokened(Output::styled(text, style))],
            tokens,
            ..Default::default()
        };
    }
    let known = session_alias
        .as_deref()
        .and_then(|a| destination(s, a).map(|d| (a, d)));
    let mut items = Vec::new();
    if o.state || known.is_none() {
        items.push(tokened(state_item(w, dim, mark)));
    }
    if let Some((alias, dest)) = known {
        let at = Draw {
            s,
            inst,
            o,
            dim,
            mark,
        };
        alias_items(&at, alias, &dest, &tokens, &mut items);
    }
    if items.is_empty() {
        // nothing selected to draw
        return Output::hidden();
    }
    finish(items)
}

/// The state word, coloured by state (`dim` while stale).
fn state_item(w: &str, dim: bool, mark: &str) -> Output {
    let style = match w {
        "on" => "good",
        "off" => "dim",
        _ => "bad",
    };
    let style = if dim { "dim" } else { style };
    let text = match w {
        "on" => "on",
        "off" => "off",
        _ => "on \u{2717}",
    };
    Output::styled(&format!("{text}{mark}"), style)
}

/// What one render draws with: the snapshot, the instance, the options and
/// the staleness already decided.
struct Draw<'a> {
    s: &'a Snapshot,
    inst: &'a Instance,
    o: &'a Opts,
    dim: bool,
    mark: &'a str,
}

/// The items for a session on a known alias: its text (with the toggle's
/// tokens), the marker and the caps, each carrying the link.
fn alias_items(
    at: &Draw,
    alias: &str,
    dest: &Dest,
    tokens: &BTreeMap<String, String>,
    items: &mut Vec<Output>,
) {
    let (s, o) = (at.s, at.o);
    let tier = s
        .cache
        .entry
        .as_ref()
        .and_then(|e| {
            e.aliases
                .iter()
                .find(|a| a.alias.eq_ignore_ascii_case(alias))
        })
        .map(|a| a.tier.clone())
        .unwrap_or_default();
    let link = o.link.as_deref().map(|t| expand(t, at.inst, alias, &tier));
    let named = o.alias && o.label && !o.compact;
    if named || o.tier {
        let (text, style) = alias_text(alias, &tier, dest, named, o.tier);
        let style = if at.dim { "dim" } else { style };
        let mut it = Output::styled(&format!("{text}{}", at.mark), style);
        it.tokens = tokens.clone();
        it.link = link.clone();
        items.push(it);
    }
    if o.marker {
        if let Dest::Router(t) = dest {
            let m = if at.inst.local_tiers.iter().any(|l| l == t) {
                "\u{2302}"
            } else {
                "\u{2601}"
            };
            let mut it = Output::text(m);
            it.link = link.clone();
            if at.dim {
                it.style = Some("dim".into());
            }
            items.push(it);
        }
    }
    if o.caps {
        let t = s.cache.tiers.iter().find(|t| t.id == tier);
        if let Some(c) = t.map(caps_text).filter(|c| !c.is_empty()) {
            let mut it = Output::styled(&c, "dim");
            it.link = link;
            items.push(it);
        }
    }
}

/// `alias[tier]` (or the tier alone when the alias is not drawn) while
/// routing is on, `tier[model]` while off, `tier[✗]` for a refused alias,
/// the alias alone without `tier`; with the state colour.
fn alias_text(
    alias: &str,
    tier: &str,
    dest: &Dest,
    named: bool,
    show_tier: bool,
) -> (String, &'static str) {
    match (dest, show_tier) {
        (Dest::Router(_), true) if named => (format!("{alias}[{tier}]"), "good"),
        (Dest::Router(_), true) => (tier.to_string(), "good"),
        (Dest::Down, true) if named => (format!("{alias}[{tier}]"), "bad"),
        (Dest::Down, true) => (tier.to_string(), "bad"),
        (Dest::Frontier(m), true) => (format!("{tier}[{m}]"), "dim"),
        (Dest::Blocked, true) => (format!("{tier}[\u{2717}]"), "bad"),
        (d, false) => (
            alias.to_string(),
            match d {
                Dest::Router(_) => "good",
                Dest::Frontier(_) => "dim",
                _ => "bad",
            },
        ),
    }
}

pub(super) fn caps_text(t: &crate::cache::Tier) -> String {
    let mut v = Vec::new();
    if t.vision == Some(true) {
        v.push("vis".to_string());
    }
    if t.tools == Some(true) {
        v.push("tools".into());
    }
    if let Some(c) = t.context {
        v.push(if c >= 1000 {
            format!("{}k", c / 1000)
        } else {
            c.to_string()
        });
    }
    v.join(" ")
}
