//! What one transcript line says, read typed where that is safe and
//! through a full JSON tree where the typed reading cannot vouch for it.

use std::borrow::Cow;
use std::marker::PhantomData;

use serde::de::value::MapAccessDeserializer;
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::parse_ts_ms;

/// What one transcript line contributes to the totals.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Facts<'a> {
    /// A main-chain `compact_boundary` system entry.
    pub compaction: bool,
    /// `Skill` tool calls in a main-chain assistant entry.
    pub skills: u64,
    /// `message.usage`, when it is an object.
    pub usage: Option<Usage<'a>>,
    /// The entry's role in a turn; main chain only.
    pub rec: Rec,
    /// `timestamp`, unix milliseconds.
    pub ts_ms: Option<i64>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rec {
    #[default]
    Other,
    /// A user entry the person typed.
    Prompt,
    /// A user entry Claude Code wrote: tool results, injected context.
    ToolResult,
    Assistant,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Usage<'a> {
    pub ts: Option<i64>,
    pub id: Cow<'a, str>,
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub output: u64,
    /// The cache write went to the 1-hour cache (`Some(false)`: 5-minute);
    /// `None` when `usage.cache_creation` gives no split.
    pub one_hour: Option<bool>,
}

/// The fields `Facts` needs, read without building a JSON tree. `C` is
/// `message.content`: its blocks on lines that mention `Skill` or are
/// user entries, skipped unread otherwise.
#[derive(Deserialize)]
struct Entry<'a, C> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    subtype: Option<Cow<'a, str>>,
    #[serde(rename = "isSidechain")]
    sidechain: Option<bool>,
    #[serde(rename = "isMeta")]
    meta: Option<bool>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow)]
    message: Option<Obj<Message<'a, C>>>,
}

#[derive(Deserialize)]
struct Message<'a, C> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    usage: Option<Obj<Counts>>,
    content: Option<C>,
}

#[derive(Deserialize)]
struct Counts {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation: Option<Obj<Split>>,
}

#[derive(Deserialize)]
struct Split {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct Block<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    name: Option<Cow<'a, str>>,
}

/// `T` read from a JSON object only; serde would also fill a struct from
/// an array, which the tree-based reading treats as no object at all.
struct Obj<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Obj<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = Obj<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Obj<T>, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(Obj)
            }
        }
        d.deserialize_map(V(PhantomData))
    }
}

/// What `message.content` holds: `Skill` calls and whether any block is
/// a tool result. Anything but an array has no blocks; array items that
/// are not objects are passed over.
#[derive(Default)]
struct Content {
    skills: u64,
    tool_result: bool,
}

/// Ignores whatever value comes; `$ok` builds the result.
macro_rules! accept_scalars {
    ($ok:expr) => {
        fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
            Ok($ok)
        }
        fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
            Ok($ok)
        }
        fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
            Ok($ok)
        }
        fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
            Ok($ok)
        }
        fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
            Ok($ok)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok($ok)
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok($ok)
        }
    };
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Content;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("message content")
            }
            accept_scalars!(Content::default());
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Content, A::Error> {
                while m.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Content::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Content, A::Error> {
                let mut c = Content::default();
                while let Some(b) = s.next_element::<AnyBlock>()? {
                    let Some(b) = b.0 else { continue };
                    match b.kind.as_deref() {
                        Some("tool_use") if b.name.as_deref() == Some("Skill") => c.skills += 1,
                        Some("tool_result") => c.tool_result = true,
                        _ => {}
                    }
                }
                Ok(c)
            }
        }
        d.deserialize_any(V)
    }
}

/// An array item: a `Block` when it is an object.
struct AnyBlock<'a>(Option<Block<'a>>);

impl<'de: 'a, 'a> Deserialize<'de> for AnyBlock<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = AnyBlock<'a>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a content block")
            }
            accept_scalars!(AnyBlock(None));
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<AnyBlock<'a>, A::Error> {
                while s.next_element::<IgnoredAny>()?.is_some() {}
                Ok(AnyBlock(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, m: A) -> Result<AnyBlock<'a>, A::Error> {
                Block::deserialize(MapAccessDeserializer::new(m)).map(|b| AnyBlock(Some(b)))
            }
        }
        d.deserialize_any(V(PhantomData))
    }
}

fn one_hour(m5: u64, h1: u64) -> Option<bool> {
    // a mixed write expires with its 5-minute part
    (m5 > 0 || h1 > 0).then_some(h1 > 0 && m5 == 0)
}

fn rec(kind: Option<&str>, main: bool, has_msg: bool, meta: bool, tool_result: bool) -> Rec {
    match kind {
        _ if !main || !has_msg => Rec::Other,
        Some("user") if meta || tool_result => Rec::ToolResult,
        Some("user") => Rec::Prompt,
        Some("assistant") => Rec::Assistant,
        _ => Rec::Other,
    }
}

/// Typed reading; `None` when the line does not fit the expected shape
/// (wrong types, duplicate keys, invalid JSON), left to `facts_tree`.
fn facts_typed<'a, C: Deserialize<'a>>(
    text: &'a str,
    boundary: bool,
    blocks: impl Fn(&C) -> (u64, bool),
) -> Option<Facts<'a>> {
    let e: Obj<Entry<'a, C>> = serde_json::from_str(text).ok()?;
    let e = e.0;
    let main = e.sidechain != Some(true);
    let compaction = boundary
        && e.kind.as_deref() == Some("system")
        && e.subtype.as_deref() == Some("compact_boundary")
        && main;
    let msg = e.message.map(|m| m.0);
    let (skills, tool_result) = msg
        .as_ref()
        .and_then(|m| m.content.as_ref())
        .map_or((0, false), blocks);
    let skills = if e.kind.as_deref() == Some("assistant") && main {
        skills
    } else {
        0
    };
    let ts_ms = e.timestamp.as_deref().and_then(parse_ts_ms);
    let rec = rec(
        e.kind.as_deref(),
        main,
        msg.is_some(),
        e.meta == Some(true),
        tool_result,
    );
    let usage = msg.and_then(|m| {
        let u = m.usage?.0;
        let split = u.cache_creation.map(|s| s.0);
        Some(Usage {
            ts: ts_ms.map(|t| t.div_euclid(1000)),
            id: m.id.unwrap_or_default(),
            input: u.input_tokens.unwrap_or(0),
            cache_creation: u.cache_creation_input_tokens.unwrap_or(0),
            cache_read: u.cache_read_input_tokens.unwrap_or(0),
            output: u.output_tokens.unwrap_or(0),
            one_hour: split.and_then(|s| {
                one_hour(
                    s.ephemeral_5m_input_tokens.unwrap_or(0),
                    s.ephemeral_1h_input_tokens.unwrap_or(0),
                )
            }),
        })
    });
    Some(Facts {
        compaction,
        skills,
        usage,
        rec,
        ts_ms,
    })
}

/// Reference reading through a full JSON tree; `None` when the line is not
/// valid JSON.
fn facts_tree(text: &str, boundary: bool, skill: bool) -> Option<Facts<'static>> {
    let v: Value = serde_json::from_str(text).ok()?;
    let main = v["isSidechain"] != true;
    let compaction =
        boundary && v["type"] == "system" && v["subtype"] == "compact_boundary" && main;
    let msg = v.get("message").filter(|m| m.is_object());
    let blocks = msg.and_then(|m| m["content"].as_array());
    let count = |t: &str, n: Option<&str>| {
        blocks.map_or(0, |b| {
            b.iter()
                .filter(|c| c["type"] == t && n.is_none_or(|n| c["name"] == n))
                .count()
        })
    };
    let skills = if skill && v["type"] == "assistant" && main {
        count("tool_use", Some("Skill")) as u64
    } else {
        0
    };
    let ts_ms = v
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_ts_ms);
    let rec = rec(
        v["type"].as_str(),
        main,
        msg.is_some(),
        v["isMeta"] == true,
        count("tool_result", None) > 0,
    );
    let usage = msg
        .and_then(|m| m.get("usage"))
        .filter(|u| u.is_object())
        .map(|u| {
            let n = |k: &str| u[k].as_u64().unwrap_or(0);
            let split = u.get("cache_creation").filter(|s| s.is_object());
            Usage {
                ts: ts_ms.map(|t| t.div_euclid(1000)),
                id: Cow::Owned(v["message"]["id"].as_str().unwrap_or_default().into()),
                input: n("input_tokens"),
                cache_creation: n("cache_creation_input_tokens"),
                cache_read: n("cache_read_input_tokens"),
                output: n("output_tokens"),
                one_hour: split.and_then(|s| {
                    let n = |k: &str| s[k].as_u64().unwrap_or(0);
                    one_hour(
                        n("ephemeral_5m_input_tokens"),
                        n("ephemeral_1h_input_tokens"),
                    )
                }),
            }
        });
    Some(Facts {
        compaction,
        skills,
        usage,
        rec,
        ts_ms,
    })
}

/// What `line` contributes. The typed reading skips what it does not
/// need without checking it; where that could accept a line the tree
/// rejects (a `\uD…` escape may be a lone surrogate) or the line does not
/// fit, the tree decides. `user`: the line reads as a user entry, so its
/// content blocks are looked at.
pub(super) fn facts(
    line: &[u8],
    usage: bool,
    boundary: bool,
    skill: bool,
    user: bool,
) -> Option<Facts<'_>> {
    // invalid UTF-8 anywhere fails the tree too
    let text = std::str::from_utf8(line).ok()?;
    let surrogate = memchr::memmem::find_iter(line, b"\\u")
        .any(|i| matches!(line.get(i + 2), Some(b'd' | b'D')));
    let typed = match (surrogate, skill || user) {
        (true, _) => None,
        (false, true) => facts_typed(text, boundary, |c: &Content| (c.skills, c.tool_result)),
        (false, false) => facts_typed(text, boundary, |_: &IgnoredAny| (0, false)),
    };
    let mut f = typed.or_else(|| facts_tree(text, boundary, skill))?;
    // the byte checks gate what they stand for
    if !skill {
        f.skills = 0;
    }
    if !user && matches!(f.rec, Rec::Prompt | Rec::ToolResult) {
        f.rec = Rec::Other;
    }
    if !usage {
        f.usage = None;
    }
    Some(f)
}
