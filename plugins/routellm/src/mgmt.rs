//! The proxy's management API as this plugin uses it: three calls and no
//! more. The provider listing carries `api-key-entries[].api-key` and
//! `headers` in plaintext; it is read into a wiped buffer and only
//! `name`, `disabled`, `priority`, `base-url` and `models[].{name,alias}`
//! are taken from it. The one write is a field-level `PATCH` that never
//! carries `base-url` (an empty one deletes the entry).

use crate::cache::{Alias, Row};
use bar_plugin::mgmt::Client as Raw;
pub use bar_plugin::mgmt::MErr;
use bar_plugin::secret::Secret;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Largest provider listing or alias listing read.
pub const CAP_LIST: usize = 128 * 1024;
pub const CAP_PATCH: usize = 8 * 1024;
/// Time for one call.
pub const T_CALL: u64 = 3;

const LIST: &str = "openai-compatibility";
const ALIASES: &str = "oauth-model-alias";

/// What is taken from one provider entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub disabled: bool,
    pub priority: i64,
    pub base_url: String,
    pub aliases: Vec<Alias>,
}

#[derive(Deserialize)]
struct RawList {
    #[serde(default, rename = "openai-compatibility")]
    items: Vec<RawItem>,
}

#[derive(Deserialize)]
struct RawItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    priority: i64,
    #[serde(default, rename = "base-url")]
    base_url: String,
    #[serde(default)]
    models: Vec<RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    alias: String,
}

#[derive(Deserialize)]
struct RawAliases {
    #[serde(default, rename = "oauth-model-alias")]
    by_channel: BTreeMap<String, Vec<RawRow>>,
}

#[derive(Deserialize)]
struct RawRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    alias: String,
}

pub struct Client<'a>(Raw<'a>);

impl<'a> Client<'a> {
    pub fn new(base: &str, key: &'a Secret) -> Result<Client<'a>, String> {
        let ua = format!("bar-routellm/{}", crate::VERSION);
        Ok(Client(Raw::new(base, key, &ua)?))
    }

    /// The entry named `name`, when the proxy has one. The whole reply is
    /// wiped when this returns.
    pub fn entry(&self, name: &str) -> Result<Option<Item>, MErr> {
        let body = self.0.request("GET", LIST, None, T_CALL, CAP_LIST)?;
        parse_entry(body.as_bytes(), name)
    }

    /// The `route-*` aliases of the claude channel.
    pub fn claude_rows(&self) -> Result<Vec<Row>, MErr> {
        let body = self.0.request("GET", ALIASES, None, T_CALL, CAP_LIST)?;
        parse_rows(body.as_bytes())
    }

    /// Sets `disabled` (and `priority`) of the entry `name`.
    pub fn set(&self, name: &str, disabled: bool, priority: Option<i64>) -> Result<(), MErr> {
        let body = patch_body(name, disabled, priority);
        self.0
            .request("PATCH", LIST, Some(&body), T_CALL, CAP_PATCH)
            .map(drop)
    }
}

/// The one write, as JSON. No `base-url`, no `models`, no keys: only the
/// fields this plugin owns.
pub fn patch_body(name: &str, disabled: bool, priority: Option<i64>) -> String {
    let mut value = serde_json::json!({ "disabled": disabled });
    if let Some(p) = priority {
        value["priority"] = p.into();
    }
    serde_json::json!({ "name": name, "value": value }).to_string()
}

pub fn parse_entry(body: &[u8], name: &str) -> Result<Option<Item>, MErr> {
    let list: RawList = serde_json::from_slice(body)
        .map_err(|_| MErr::Transient("response is not the provider listing".into()))?;
    Ok(list
        .items
        .into_iter()
        .find(|i| i.name == name)
        .map(|i| Item {
            name: i.name,
            disabled: i.disabled,
            priority: i.priority,
            base_url: i.base_url,
            aliases: i
                .models
                .into_iter()
                .filter(|m| !m.alias.is_empty())
                .map(|m| Alias {
                    alias: m.alias,
                    tier: m.name,
                })
                .collect(),
        }))
}

pub fn parse_rows(body: &[u8]) -> Result<Vec<Row>, MErr> {
    let a: RawAliases = serde_json::from_slice(body)
        .map_err(|_| MErr::Transient("response is not the alias listing".into()))?;
    Ok(a.by_channel
        .get("claude")
        .into_iter()
        .flatten()
        .filter(|r| r.alias.starts_with("route-"))
        .map(|r| Row {
            alias: r.alias.clone(),
            model: r.name.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"{"openai-compatibility":[
        {"name":"other","priority":1,"disabled":false,"base-url":"http://x/v1","models":[]},
        {"name":"routellm","priority":1000,"disabled":true,"base-url":"http://127.0.0.1:6060/v1",
         "api-key-entries":[{"api-key":"SECRET-KEY-1","auth-index":"ab"}],
         "headers":{"X-Secret":"SECRET-HEADER"},
         "models":[{"name":"auto","alias":"route-auto"},{"name":"vision","alias":"route-vision"},{"name":"x","alias":""}]}]}"#;

    #[test]
    fn the_entry_keeps_only_what_is_declared() {
        let it = parse_entry(LISTING.as_bytes(), "routellm")
            .unwrap()
            .unwrap();
        assert!(it.disabled);
        assert_eq!(it.priority, 1000);
        assert_eq!(it.base_url, "http://127.0.0.1:6060/v1");
        assert_eq!(
            it.aliases,
            [
                Alias {
                    alias: "route-auto".into(),
                    tier: "auto".into()
                },
                Alias {
                    alias: "route-vision".into(),
                    tier: "vision".into()
                }
            ]
        );
        assert!(!format!("{it:?}").contains("SECRET"));
        assert_eq!(parse_entry(LISTING.as_bytes(), "nope").unwrap(), None);
        assert!(parse_entry(b"not json", "x").is_err());
    }

    #[test]
    fn the_patch_carries_only_disabled_and_priority() {
        let b: serde_json::Value =
            serde_json::from_str(&patch_body("routellm", true, None)).unwrap();
        assert_eq!(
            b,
            serde_json::json!({"name":"routellm","value":{"disabled":true}})
        );
        let b: serde_json::Value =
            serde_json::from_str(&patch_body("routellm", false, Some(1000))).unwrap();
        assert_eq!(
            b,
            serde_json::json!({"name":"routellm","value":{"disabled":false,"priority":1000}})
        );
        for s in [
            patch_body("routellm", false, Some(5)),
            patch_body("a", true, None),
        ] {
            assert!(!s.contains("base-url") && !s.contains("models") && !s.contains("key"));
        }
    }

    #[test]
    fn claude_rows_are_the_route_aliases_only() {
        let body = r#"{"oauth-model-alias":{"claude":[
            {"name":"claude-sonnet-5-5","alias":"route-auto","fork":true},
            {"name":"claude-opus-5-5","alias":"opus-short"}],
            "codex":[{"name":"gpt-5","alias":"route-x"}]}}"#;
        let r = parse_rows(body.as_bytes()).unwrap();
        assert_eq!(
            r,
            [Row {
                alias: "route-auto".into(),
                model: "claude-sonnet-5-5".into()
            }]
        );
        assert!(parse_rows(b"{}").unwrap().is_empty());
    }
}
