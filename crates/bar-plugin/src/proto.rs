//! The plugin protocol's JSON shapes: what `--ext-info` declares, what a
//! render receives on stdin and what it prints, plus the sanitising rules
//! bar applies to output text.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Most items a render output may carry.
pub const MAX_ITEMS: usize = 16;
/// Longest token value, in characters.
pub const MAX_TOKEN: usize = 64;

/// `--ext-info` output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub options: BTreeMap<String, OptSpec>,
    #[serde(default)]
    pub ttl: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// `"session"`: cache per Claude Code session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    /// Environment and session values that split the cache and reach the
    /// render input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Inputs>,
    /// Render time limit, e.g. `"15s"` (1 to 15 s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    /// Actions bindable as clicks, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, ActionSpec>,
    /// Other names `bar <name>` answers to: up to 4 of `[a-z0-9-]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptSpec {
    /// `int`, `string`, `bool` or `enum`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, rename = "enum", skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

impl OptSpec {
    pub fn string() -> OptSpec {
        OptSpec {
            kind: "string".into(),
            ..Default::default()
        }
    }
    pub fn int(default: i64) -> OptSpec {
        OptSpec {
            kind: "int".into(),
            choices: vec![],
            default: Some(default.into()),
        }
    }
    pub fn bool() -> OptSpec {
        OptSpec {
            kind: "bool".into(),
            ..Default::default()
        }
    }
    pub fn choice(choices: &[&str], default: &str) -> OptSpec {
        OptSpec {
            kind: "enum".into(),
            choices: choices.iter().map(|s| s.to_string()).collect(),
            default: Some(default.into()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inputs {
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub session: Vec<String>,
    /// Ask for the user's `account` rules (`accounts` in the render and
    /// action input, `BAR_ACCOUNTS` for `bar <name>`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub accounts: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One of the user's `account "<glob>" <label> <color>` lines. The glob
/// can hold an email address: match it where the address is visible and
/// keep only the label and colour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AccountRule {
    #[serde(default)]
    pub glob: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color: String,
}

/// Name of the environment variable `bar <name>` sets for a plugin that
/// declared `inputs.accounts`: a JSON array of [`AccountRule`].
pub const ACCOUNTS_ENV: &str = "BAR_ACCOUNTS";

/// The rules in a JSON array (`accounts` of an input, or the value of
/// [`ACCOUNTS_ENV`]); empty when absent or malformed. At most 64.
pub fn parse_accounts(v: &Value) -> Vec<AccountRule> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| serde_json::from_value::<AccountRule>(r.clone()).ok())
                .filter(|r| !r.glob.is_empty())
                .take(64)
                .collect()
        })
        .unwrap_or_default()
}

/// The rules `bar <name>` put in the environment.
pub fn accounts_from_env() -> Vec<AccountRule> {
    std::env::var(ACCOUNTS_ENV)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| parse_accounts(&v))
        .unwrap_or_default()
}

/// The `accounts` of an action's stdin.
pub fn accounts_of_action(stdin: &str) -> Vec<AccountRule> {
    serde_json::from_str::<Value>(stdin)
        .map(|v| parse_accounts(&v["accounts"]))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionSpec {
    pub description: String,
    #[serde(default)]
    pub tokens: Vec<String>,
    #[serde(default)]
    pub confirm: bool,
    /// Seconds the action may run (default 10, at most 120): for one that
    /// waits on the user.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub timeout: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// What a render receives on stdin. Unknown fields are ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RenderInput {
    #[serde(default)]
    pub options: Map<String, Value>,
    #[serde(default)]
    pub session: SessionInput,
    #[serde(default)]
    pub columns: Option<u32>,
    #[serde(default)]
    pub config: Map<String, Value>,
    #[serde(default)]
    pub toggles: Map<String, Value>,
    /// Values of the environment variables `--ext-info` `inputs.env` named.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The user's `account` lines, when `inputs.accounts` is declared.
    #[serde(default)]
    pub accounts: Vec<AccountRule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionInput {
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub session_id: String,
}

impl RenderInput {
    pub fn opt_str(&self, key: &str) -> Option<&str> {
        self.options.get(key).and_then(Value::as_str)
    }
    pub fn opt_int(&self, key: &str) -> Option<i64> {
        self.options.get(key).and_then(Value::as_i64)
    }
    pub fn opt_bool(&self, key: &str) -> Option<bool> {
        self.options.get(key).and_then(Value::as_bool)
    }
}

/// A usage window: percent used (0 to 100) and the reset time (Unix s).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

/// Usage windows as the status line's `rate_limits`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
}

/// The account a proxy prefers, for `#{account}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    /// Set when a user `account` line matched: bar then draws `label` as
    /// is, in this colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A render output (also one item of `items`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Output {
    /// Required unless `items` is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Colour of `label`: a code, `#rrggbb` or an ANSI name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// `good`, `warn`, `bad` or `dim`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// One output per account (at most `MAX_ITEMS`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Output>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
    /// Values an action binding may use (`account`, `provider`, ...).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
}

impl Output {
    /// Plain text.
    pub fn text(t: &str) -> Output {
        Output {
            text: Some(sanitize(t)),
            ..Default::default()
        }
    }
    /// Nothing to show: bar omits the segment.
    pub fn hidden() -> Output {
        Output::text("")
    }
    /// Text with a style.
    pub fn styled(t: &str, style: &str) -> Output {
        Output {
            style: Some(style.into()),
            ..Output::text(t)
        }
    }

    /// Sanitises every string, drops invalid tokens and surplus items.
    pub fn finish(mut self) -> Output {
        let s = |o: &mut Option<String>| *o = o.take().map(|v| sanitize(&v));
        s(&mut self.text);
        s(&mut self.alt);
        s(&mut self.label);
        self.tokens
            .retain(|k, v| valid_token_name(k) && valid_token(v));
        self.items.truncate(MAX_ITEMS);
        self.items = self.items.into_iter().map(Output::finish).collect();
        if let Some(a) = &mut self.account {
            a.id = sanitize(&a.id);
            a.label = sanitize(&a.label);
            a.color = a.color.take().filter(|c| valid_color(c));
        }
        self.label_color = self.label_color.take().filter(|c| valid_color(c));
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"text":""}"#.into())
    }
}

/// `--check` output: what is wrong, in plain sentences.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckReport {
    pub warnings: Vec<String>,
}

/// A colour as bar reads one: a code, `#rrggbb` or an ANSI name.
pub fn valid_color(c: &str) -> bool {
    (1..=16).contains(&c.len())
        && c.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"#-".contains(&b))
}

/// `[A-Za-z0-9._@-]`, 1 to `MAX_TOKEN` characters.
pub fn valid_token(v: &str) -> bool {
    (1..=MAX_TOKEN).contains(&v.len())
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._@-".contains(&b))
}

fn valid_token_name(k: &str) -> bool {
    (1..=MAX_TOKEN).contains(&k.len())
        && k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

/// Text as bar draws it: control characters dropped (tab and line break
/// become spaces), plus line and paragraph separators and format
/// characters (bidi marks and overrides, zero-width spaces, soft hyphen,
/// BOM; the joiners U+200C and U+200D are kept).
pub fn sanitize(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            '\u{2028}' | '\u{2029}' | '\u{00AD}' | '\u{FEFF}' | '\u{061C}' => None,
            '\u{200B}' | '\u{200E}' | '\u{200F}' => None,
            '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_drops_what_bar_would() {
        assert_eq!(sanitize("a\u{1b}[31mb\tc\nd"), "a[31mb c d");
        assert_eq!(sanitize("x\u{202E}y\u{200B}z\u{200D}"), "xyz\u{200D}");
    }

    #[test]
    fn tokens_are_checked() {
        assert!(valid_token("a1.b_c@d-e"));
        assert!(!valid_token(""));
        assert!(!valid_token("a b"));
        assert!(!valid_token(&"a".repeat(65)));
    }

    #[test]
    fn finish_cleans_the_output() {
        let mut o = Output::text("hi\u{1b}");
        o.tokens.insert("account".into(), "ok-1".into());
        o.tokens.insert("provider".into(), "bad value".into());
        o.items = (0..20).map(|i| Output::text(&i.to_string())).collect();
        let o = o.finish();
        assert_eq!(o.text.as_deref(), Some("hi"));
        assert_eq!(o.tokens.len(), 1);
        assert_eq!(o.items.len(), MAX_ITEMS);
    }

    #[test]
    fn json_shapes_match_the_protocol() {
        let mut o = Output::text("x");
        o.usage = Some(Usage {
            five_hour: None,
            seven_day: Some(Window {
                used_percentage: 12.5,
                resets_at: 1800400000,
            }),
        });
        assert_eq!(
            o.to_json(),
            r#"{"text":"x","usage":{"seven_day":{"used_percentage":12.5,"resets_at":1800400000}}}"#
        );
        let mut i = ExtInfo {
            name: "p".into(),
            version: "1".into(),
            ..Default::default()
        };
        i.actions.insert(
            "use".into(),
            ActionSpec {
                description: "d".into(),
                tokens: vec!["account".into()],
                confirm: false,
                timeout: 0,
            },
        );
        let v: Value = serde_json::from_str(&serde_json::to_string(&i).unwrap()).unwrap();
        assert_eq!(v["actions"]["use"]["tokens"][0], "account");
        assert!(v.get("inputs").is_none() && v.get("timeout").is_none());
    }

    #[test]
    fn render_input_ignores_unknown_fields() {
        let i: RenderInput = serde_json::from_str(
            r#"{"options":{"n":3},"session":{"model_id":"m","x":1},"env":{"A":"b"},"extra":1}"#,
        )
        .unwrap();
        assert_eq!(i.opt_int("n"), Some(3));
        assert_eq!(i.session.model_id, "m");
        assert_eq!(i.env["A"], "b");
    }
}
