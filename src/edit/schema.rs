//! What the editor offers for each placeholder: a one-line description and
//! its options, from the core option table or a plugin's `--ext-info`.

use crate::check::{core_options, Val};
use crate::plugin::{ExtInfo, OptType};

/// How an option is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// One of these values.
    Choice(Vec<String>),
    /// Comma list of these items.
    List(Vec<String>),
    /// Whole number; `min` is the smallest allowed.
    Int {
        min: usize,
    },
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opt {
    pub key: String,
    pub kind: Kind,
    /// Default value, when the schema states one.
    pub default: Option<String>,
}

impl Opt {
    fn new(key: &str, kind: Kind) -> Opt {
        Opt {
            key: key.into(),
            kind,
            default: None,
        }
    }

    /// Why `raw` is not a valid value, if it is not.
    pub fn invalid(&self, raw: &str) -> Option<String> {
        let ok = match &self.kind {
            Kind::Choice(c) => c.iter().any(|v| v == raw),
            Kind::List(items) => raw.split(',').all(|i| items.iter().any(|v| v == i.trim())),
            Kind::Int { min } => raw.trim().parse::<usize>().is_ok_and(|n| n >= *min),
            Kind::Text => true,
        };
        (!ok).then(|| match &self.kind {
            Kind::Choice(c) => format!("one of {}", c.join(", ")),
            Kind::List(i) => format!("comma list of {}", i.join(", ")),
            Kind::Int { min } => format!("a whole number from {min}"),
            Kind::Text => String::new(),
        })
    }
}

pub use crate::describe::describe;

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Options bar handles itself on every placeholder.
fn generic() -> Vec<Opt> {
    vec![
        Opt::new("icon", Kind::Text),
        Opt::new("bg", Kind::Text),
        Opt::new("fg", Kind::Text),
        Opt::new("bold", Kind::Choice(owned(&["on", "off"]))),
        Opt::new("gradient", Kind::Text),
    ]
}

/// Options of core placeholder `name`; `None` when it is not one.
pub fn core(name: &str) -> Option<Vec<Opt>> {
    let mut out: Vec<Opt> = core_options(name)?
        .iter()
        .map(|(k, v)| {
            let kind = match v {
                Val::Any => Kind::Text,
                Val::Int => Kind::Int { min: 0 },
                Val::Pos => Kind::Int { min: 1 },
                Val::OneOf(c) => Kind::Choice(owned(c)),
                Val::List(c) => Kind::List(owned(c)),
            };
            Opt::new(k, kind)
        })
        .collect();
    out.extend(generic());
    Some(out)
}

/// Options of a plugin, from its `--ext-info` schema (none without one),
/// plus the generic ones and `click`.
pub fn plugin(info: Option<&ExtInfo>) -> Vec<Opt> {
    let mut out: Vec<Opt> = info
        .map(|i| i.options.iter().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|(k, spec)| {
            let kind = match spec.kind {
                OptType::Int => Kind::Int { min: 0 },
                OptType::String => Kind::Text,
                OptType::Bool => Kind::Choice(owned(&["true", "false"])),
                OptType::Enum => Kind::Choice(spec.choices.clone()),
            };
            Opt {
                key: k.clone(),
                kind,
                default: spec.default.as_ref().map(|d| match d.as_str() {
                    Some(s) => s.to_string(),
                    None => d.to_string(),
                }),
            }
        })
        .collect();
    out.extend(generic());
    out.push(Opt::new("click", Kind::Choice(owned(&["toggle", "none"]))));
    out
}
