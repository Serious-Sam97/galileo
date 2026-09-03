//! Attributes are the heart of a wide-event store: any key, any value, all queryable.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A single attribute value. Mirrors the OTLP `AnyValue` shape but stays independent of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    Str(String),
    Bool(bool),
    Int(i64),
    Float(f64),
    Bytes(Vec<u8>),
    Array(Vec<AttributeValue>),
    Map(IndexMap<String, AttributeValue>),
}

impl AttributeValue {
    /// The string form stored in ClickHouse's `Map(String, String)` column. Every value has one,
    /// so every attribute is filterable and groupable even if it was not numeric.
    pub fn to_string_repr(&self) -> String {
        match self {
            AttributeValue::Str(s) => s.clone(),
            AttributeValue::Bool(b) => b.to_string(),
            AttributeValue::Int(i) => i.to_string(),
            AttributeValue::Float(f) => {
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    format!("{}", *f as i64)
                } else {
                    f.to_string()
                }
            }
            AttributeValue::Bytes(b) => hex::encode(b),
            AttributeValue::Array(_) | AttributeValue::Map(_) => {
                serde_json::to_string(self).unwrap_or_default()
            }
        }
    }

    /// Numeric form, if the value is a number or a bool. Stored in a separate numeric map so
    /// percentiles and sums work without casting at query time.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            AttributeValue::Int(i) => Some(*i as f64),
            AttributeValue::Float(f) => Some(*f),
            AttributeValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttributeValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            AttributeValue::Int(i) => Some(*i),
            AttributeValue::Float(f) => Some(*f as i64),
            AttributeValue::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }
}

impl From<&str> for AttributeValue {
    fn from(s: &str) -> Self {
        AttributeValue::Str(s.to_owned())
    }
}
impl From<String> for AttributeValue {
    fn from(s: String) -> Self {
        AttributeValue::Str(s)
    }
}
impl From<i64> for AttributeValue {
    fn from(v: i64) -> Self {
        AttributeValue::Int(v)
    }
}
impl From<i32> for AttributeValue {
    fn from(v: i32) -> Self {
        AttributeValue::Int(v as i64)
    }
}
impl From<u64> for AttributeValue {
    fn from(v: u64) -> Self {
        AttributeValue::Int(v as i64)
    }
}
impl From<f64> for AttributeValue {
    fn from(v: f64) -> Self {
        AttributeValue::Float(v)
    }
}
impl From<bool> for AttributeValue {
    fn from(v: bool) -> Self {
        AttributeValue::Bool(v)
    }
}

/// Ordered so that serialisation is deterministic (tests, hashing, display).
pub type Attributes = IndexMap<String, AttributeValue>;

/// String column values and numeric column values, as produced by [`split_for_storage`].
pub type StorageColumns = (Vec<(String, String)>, Vec<(String, f64)>);

/// Split attributes into the two column families the store uses: every value as a string,
/// and numeric values additionally as f64.
pub fn split_for_storage(attrs: &Attributes) -> StorageColumns {
    let mut strs = Vec::with_capacity(attrs.len());
    let mut nums = Vec::new();
    for (k, v) in attrs {
        strs.push((k.clone(), v.to_string_repr()));
        if let Some(n) = v.as_f64() {
            nums.push((k.clone(), n));
        }
    }
    (strs, nums)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_repr_is_stable() {
        assert_eq!(AttributeValue::Float(3.0).to_string_repr(), "3");
        assert_eq!(AttributeValue::Float(3.5).to_string_repr(), "3.5");
        assert_eq!(AttributeValue::Bool(true).to_string_repr(), "true");
        assert_eq!(AttributeValue::Bytes(vec![0xde, 0xad]).to_string_repr(), "dead");
        assert_eq!(
            AttributeValue::Array(vec![1i64.into(), "a".into()]).to_string_repr(),
            "[1,\"a\"]"
        );
    }

    #[test]
    fn split_keeps_numbers_separately() {
        let mut a = Attributes::new();
        a.insert("http.status_code".into(), 200i64.into());
        a.insert("http.route".into(), "/x".into());
        let (s, n) = split_for_storage(&a);
        assert_eq!(s.len(), 2);
        assert_eq!(n, vec![("http.status_code".to_string(), 200.0)]);
    }
}
