use super::*;

/// Which proxy-config layout `setup` prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `config-version: 8`: `api-keys.openai-compatibility`, `oauth.model-alias`, `keys`.
    V8,
    /// Flat file: `openai-compatibility`, `oauth-model-alias`, `api-key-entries`.
    Legacy,
}

impl Layout {
    pub fn parse(s: &str) -> Option<Layout> {
        match s {
            "v8" => Some(Layout::V8),
            "legacy" => Some(Layout::Legacy),
            _ => None,
        }
    }
}

/// The proxy-config snippet `setup` prints; nothing is written.
pub fn setup_snippet(inst: &Instance, c: &Cache, layout: Layout) -> Vec<String> {
    let aliases: Vec<(String, String)> = match &c.entry {
        Some(e) if !e.aliases.is_empty() => e
            .aliases
            .iter()
            .map(|a| (a.alias.clone(), a.tier.clone()))
            .collect(),
        _ => ["auto", "vision", "private"]
            .iter()
            .map(|t| (format!("route-{t}"), t.to_string()))
            .collect(),
    };
    // (indent of the provider list, key-list field, claude rows' indent)
    let (mut o, keys_field) = match layout {
        Layout::V8 => (
            vec![
                "# Add to the proxy's config (v8 layout: `config-version: 8`). Merge each".into(),
                "# section into the file's existing one; do not repeat a top-level key.".into(),
                "oauth:".into(),
                "  model-alias:".into(),
                "    claude:".into(),
            ],
            "keys",
        ),
        Layout::Legacy => (
            vec![
                "# Add to the proxy's config (legacy layout, no `config-version: 8`). Merge each"
                    .into(),
                "# section into the file's existing one; do not repeat a top-level key.".into(),
                "oauth-model-alias:".into(),
                "  claude:".into(),
            ],
            "api-key-entries",
        ),
    };
    let (pad, row_pad) = match layout {
        Layout::V8 => ("    ", "      "),
        Layout::Legacy => ("  ", "    "),
    };
    for (alias, tier) in &aliases {
        // route-private stays without a row: OFF then fails closed
        if tier == "private" {
            continue;
        }
        let model = inst
            .off_map
            .iter()
            .find(|(a, _)| a == alias)
            .map_or("claude-sonnet-5-5", |(_, m)| m.as_str());
        o.push(format!(
            "{row_pad}- {{name: {model}, alias: {alias}, fork: true}}"
        ));
    }
    if layout == Layout::V8 {
        o.push("api-keys:".into());
    }
    o.push(match layout {
        Layout::V8 => "  openai-compatibility:".into(),
        Layout::Legacy => "openai-compatibility:".into(),
    });
    let item = format!("{pad}- name: {}", inst.provider);
    let f = format!("{pad}  ");
    o.push(item);
    // routing stays OFF until `bar rtellm on`
    o.push(format!("{f}disabled: true"));
    o.push(format!("{f}base-url: {}/v1", inst.routellm));
    o.push(format!("{f}priority: {}", inst.on_priority));
    // without it, a provider that failed is skipped for about a minute
    // after it recovers
    o.push(format!("{f}disable-cooling: true"));
    o.push(format!("{f}{keys_field}:"));
    o.push(format!("{f}  - api-key: \"unused\""));
    o.push(format!("{f}models:"));
    for (alias, tier) in &aliases {
        o.push(format!("{f}  - {{name: {tier}, alias: {alias}}}"));
    }
    o.push("# `fork: true` keeps the original model id available to RouteLLM's own calls.".into());
    o.push("# No claude row for route-private: with routing off, that alias errors instead of leaving this host.".into());
    o.push(
        "# The api-key value is a placeholder; put the key RouteLLM expects, if it checks one."
            .into(),
    );
    o.push("# The provider is `disabled: true`: routing stays OFF until `bar rtellm on`.".into());
    if layout == Layout::V8 {
        o.push("# The plugin's first toggle rewrites a v8-layout file (comments are dropped); keep a backup.".into());
    }
    o
}
