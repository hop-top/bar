use super::*;

/// The placeholder's options.
#[derive(Debug, Clone, PartialEq)]
pub struct Opts {
    pub state: bool,
    pub alias: bool,
    pub tier: bool,
    /// The `\u{2302}`/`\u{2601}` item after the tier.
    pub marker: bool,
    pub caps: bool,
    /// Draw the alias item (`label=alias`, and not `compact`).
    pub label: bool,
    pub compact: bool,
    pub stale: Stale,
    pub link: Option<String>,
    /// Hide unless the session's model is a `route-*` alias.
    pub routed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stale {
    Mark,
    Dim,
    Hide,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            state: false,
            alias: true,
            tier: true,
            marker: false,
            caps: false,
            label: true,
            compact: false,
            stale: Stale::Mark,
            link: None,
            routed: false,
        }
    }
}

impl Opts {
    pub fn from_input(i: &RenderInput) -> Opts {
        let mut o = Opts::default();
        if let Some(list) = i.opt_str("show") {
            let has = |k: &str| list.split(',').any(|p| p.trim().eq_ignore_ascii_case(k));
            // a list naming none of the parts keeps the default
            if ["state", "alias", "tier", "marker", "caps"]
                .iter()
                .any(|k| has(k))
            {
                (o.state, o.alias, o.tier, o.marker, o.caps) = (
                    has("state"),
                    has("alias"),
                    has("tier"),
                    has("marker"),
                    has("caps"),
                );
            }
        }
        o.label = i.opt_str("label") != Some("none");
        o.compact = i.opt_bool("compact").unwrap_or(false);
        o.stale = match i.opt_str("stale") {
            Some("dim") => Stale::Dim,
            Some("hide") => Stale::Hide,
            _ => Stale::Mark,
        };
        o.link = i
            .opt_str("link")
            .filter(|l| !l.is_empty())
            .map(String::from);
        o.routed = i.opt_bool("routed").unwrap_or(false);
        o
    }
}

/// `route-auto` for `route-auto` or `team/route-auto`: the alias a session
/// model id names, as the proxy knows it.
pub fn alias_of_model(model_id: &str) -> Option<String> {
    let m = model_id.rsplit('/').next().unwrap_or(model_id);
    (m.len() > 6 && m.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("route-")))
        .then(|| m.to_ascii_lowercase())
}

/// Where a request of an alias goes in the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    /// RouteLLM, to this tier.
    Router(String),
    /// A frontier model, through the proxy's own alias row.
    Frontier(String),
    /// Nowhere: the alias has no row and RouteLLM is not serving it.
    Blocked,
    /// Routing is on and RouteLLM is not answering: where the proxy sends
    /// the request then is not known to the plugin.
    Down,
}

/// The destination of `alias` (the proxy's alias of the session's model),
/// from the cached provider entry; `None` when the alias is not one the
/// provider exposes or the state is not known.
pub fn destination(s: &Snapshot, alias: &str) -> Option<Dest> {
    let w = state_word(s);
    if !matches!(w, "on" | "off" | "down") {
        return None;
    }
    let a = s
        .cache
        .entry
        .as_ref()?
        .aliases
        .iter()
        .find(|a| a.alias.eq_ignore_ascii_case(alias))?;
    let row = s
        .cache
        .rows
        .iter()
        .find(|r| r.alias.eq_ignore_ascii_case(alias));
    Some(match (w, row) {
        ("on", _) => Dest::Router(a.tier.clone()),
        ("down", _) => Dest::Down,
        (_, Some(r)) => Dest::Frontier(r.model.clone()),
        _ => Dest::Blocked,
    })
}
