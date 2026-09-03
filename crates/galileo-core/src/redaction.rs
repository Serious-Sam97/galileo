//! Redaction runs at ingest, before anything touches disk. Rules are per project and apply to
//! span/log/metric attributes, log bodies and gateway prompts/completions.
//!
//! Two shapes of rule:
//! * key rules: match an attribute key (glob-ish: `*` wildcard) and drop or hash or mask it;
//! * value rules: a regex applied to every string value (and bodies), replaced with a mask.
//!
//! Built-in defaults cover the obvious: emails, credit-card-looking numbers, bearer tokens,
//! `password`/`secret`/`authorization` keys.

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{AttributeValue, Attributes};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionAction {
    /// Remove the attribute entirely.
    Drop,
    /// Replace the value with `[REDACTED]` (keeps the key visible so you know it existed).
    Mask,
    /// Replace the value with a short stable hash so it can still be grouped/counted.
    Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RedactionRule {
    /// Applies to attribute keys. `pattern` supports `*` as a wildcard, matched case-insensitively.
    Key { pattern: String, action: RedactionAction },
    /// Applies to every string value and log/prompt bodies. `regex` is a Rust regex; every
    /// match is replaced with `replacement` (default `[REDACTED]`).
    Value {
        regex: String,
        #[serde(default = "default_replacement")]
        replacement: String,
    },
}

fn default_replacement() -> String {
    "[REDACTED]".to_owned()
}

pub const MASK: &str = "[REDACTED]";

enum Compiled {
    Key { re: Regex, action: RedactionAction },
    Value { re: Regex, replacement: String },
}

/// Compiled rule set. Cheap to clone via Arc at the call sites; build once per project and
/// rebuild when rules change.
pub struct Redactor {
    rules: Vec<Compiled>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} rules)", self.rules.len())
    }
}

impl Redactor {
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// The rules every project starts with.
    pub fn default_rules() -> Vec<RedactionRule> {
        vec![
            RedactionRule::Key { pattern: "*password*".into(), action: RedactionAction::Drop },
            RedactionRule::Key { pattern: "*secret*".into(), action: RedactionAction::Drop },
            RedactionRule::Key { pattern: "*authorization*".into(), action: RedactionAction::Mask },
            RedactionRule::Key { pattern: "*api_key*".into(), action: RedactionAction::Mask },
            RedactionRule::Key { pattern: "*apikey*".into(), action: RedactionAction::Mask },
            RedactionRule::Key { pattern: "http.request.header.cookie".into(), action: RedactionAction::Drop },
            RedactionRule::Key { pattern: "http.request.header.set-cookie".into(), action: RedactionAction::Drop },
            RedactionRule::Value {
                regex: r"(?i)bearer\s+[a-z0-9\-._~+/]+=*".into(),
                replacement: "Bearer [REDACTED]".into(),
            },
            RedactionRule::Value {
                regex: r"\b\d(?:[ -]?\d){12,15}\b".into(),
                replacement: "[CARD]".into(),
            },
        ]
    }

    pub fn compile(rules: &[RedactionRule]) -> Result<Self, regex::Error> {
        let mut out = Vec::with_capacity(rules.len());
        for r in rules {
            out.push(match r {
                RedactionRule::Key { pattern, action } => Compiled::Key {
                    re: Regex::new(&glob_to_regex(pattern))?,
                    action: action.clone(),
                },
                RedactionRule::Value { regex, replacement } => Compiled::Value {
                    re: Regex::new(regex)?,
                    replacement: replacement.clone(),
                },
            });
        }
        Ok(Self { rules: out })
    }

    pub fn with_defaults(extra: &[RedactionRule]) -> Result<Self, regex::Error> {
        let mut all = Self::default_rules();
        all.extend_from_slice(extra);
        Self::compile(&all)
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Redact a free-text body (log body, prompt, completion). Only value rules apply.
    pub fn redact_text(&self, text: &str) -> String {
        let mut cur = std::borrow::Cow::Borrowed(text);
        for rule in &self.rules {
            if let Compiled::Value { re, replacement } = rule {
                if re.is_match(&cur) {
                    cur = std::borrow::Cow::Owned(re.replace_all(&cur, replacement.as_str()).into_owned());
                }
            }
        }
        cur.into_owned()
    }

    /// Redact an attribute map in place.
    pub fn redact_attrs(&self, attrs: &mut Attributes) {
        if self.rules.is_empty() {
            return;
        }
        let keys: Vec<String> = attrs.keys().cloned().collect();
        for key in keys {
            let mut action: Option<&RedactionAction> = None;
            for rule in &self.rules {
                if let Compiled::Key { re, action: a } = rule {
                    if re.is_match(&key) {
                        action = Some(a);
                        // Drop wins over everything else.
                        if matches!(a, RedactionAction::Drop) {
                            break;
                        }
                    }
                }
            }
            match action {
                Some(RedactionAction::Drop) => {
                    attrs.shift_remove(&key);
                    continue;
                }
                Some(RedactionAction::Mask) => {
                    attrs.insert(key, AttributeValue::Str(MASK.into()));
                    continue;
                }
                Some(RedactionAction::Hash) => {
                    let h = short_hash(&attrs[&key].to_string_repr());
                    attrs.insert(key, AttributeValue::Str(h));
                    continue;
                }
                None => {}
            }
            if let Some(v) = attrs.get_mut(&key) {
                self.redact_value(v);
            }
        }
    }

    fn redact_value(&self, v: &mut AttributeValue) {
        match v {
            AttributeValue::Str(s) => {
                let r = self.redact_text(s);
                if r != *s {
                    *s = r;
                }
            }
            AttributeValue::Array(items) => items.iter_mut().for_each(|i| self.redact_value(i)),
            AttributeValue::Map(m) => m.values_mut().for_each(|i| self.redact_value(i)),
            _ => {}
        }
    }
}

fn glob_to_regex(glob: &str) -> String {
    let mut re = String::from("(?i)^");
    for ch in glob.chars() {
        match ch {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    re
}

/// Stable, non-reversible, short. FNV-1a 64 is enough for grouping; this is not a security
/// boundary, dropping is.
fn short_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("h:{:016x}", h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> Attributes {
        pairs.iter().map(|(k, v)| (k.to_string(), AttributeValue::from(*v))).collect()
    }

    #[test]
    fn default_rules_drop_and_mask_keys() {
        let r = Redactor::with_defaults(&[]).unwrap();
        let mut a = attrs(&[
            ("db.password", "hunter2"),
            ("http.request.header.authorization", "Bearer abc"),
            ("http.route", "/users"),
        ]);
        r.redact_attrs(&mut a);
        assert!(a.get("db.password").is_none());
        assert_eq!(a["http.request.header.authorization"].as_str(), Some(MASK));
        assert_eq!(a["http.route"].as_str(), Some("/users"));
    }

    #[test]
    fn value_rules_apply_to_strings_and_text() {
        let r = Redactor::with_defaults(&[]).unwrap();
        let mut a = attrs(&[("note", "token is Bearer abc.def-123 ok"), ("card", "4111 1111 1111 1111")]);
        r.redact_attrs(&mut a);
        assert_eq!(a["note"].as_str(), Some("token is Bearer [REDACTED] ok"));
        assert_eq!(a["card"].as_str(), Some("[CARD]"));
        assert_eq!(r.redact_text("pay 4111111111111111 now"), "pay [CARD] now");
    }

    #[test]
    fn hash_keeps_groupability() {
        let r = Redactor::compile(&[RedactionRule::Key {
            pattern: "user.email".into(),
            action: RedactionAction::Hash,
        }])
        .unwrap();
        let mut a = attrs(&[("user.email", "a@b.c")]);
        let mut b = attrs(&[("user.email", "a@b.c")]);
        r.redact_attrs(&mut a);
        r.redact_attrs(&mut b);
        assert_eq!(a["user.email"], b["user.email"]);
        assert!(a["user.email"].as_str().unwrap().starts_with("h:"));
    }

    #[test]
    fn custom_email_rule() {
        let r = Redactor::compile(&[RedactionRule::Value {
            regex: r"[\w.+-]+@[\w-]+\.[\w.]+".into(),
            replacement: "[EMAIL]".into(),
        }])
        .unwrap();
        assert_eq!(r.redact_text("mail sam@example.com pls"), "mail [EMAIL] pls");
    }

    #[test]
    fn glob_is_anchored_and_case_insensitive() {
        assert_eq!(glob_to_regex("*Password*"), "(?i)^.*Password.*$");
        let re = Regex::new(&glob_to_regex("http.route")).unwrap();
        assert!(re.is_match("HTTP.ROUTE"));
        assert!(!re.is_match("http.route.extra"));
    }
}
