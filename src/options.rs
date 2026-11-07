//! Insertion-ordered key/value options.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options(Vec<(String, String)>);

impl Options {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Replace in place when present, else append.
    pub fn set(&mut self, key: &str, value: &str) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.0.push((key.to_string(), value.to_string())),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `defaults` first, then `self` overriding or appending.
    pub fn merged_over(&self, defaults: &Options) -> Options {
        let mut out = defaults.clone();
        for (k, v) in &self.0 {
            out.set(k, v);
        }
        out
    }
}
