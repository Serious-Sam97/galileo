use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Every stored row belongs to exactly one project. Projects belong to organizations, but
/// the event store only ever needs the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ProjectId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// W3C trace id: 16 bytes, rendered as 32 lowercase hex chars.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct TraceId(pub [u8; 16]);

/// W3C span id: 8 bytes, rendered as 16 lowercase hex chars.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct SpanId(pub [u8; 8]);

macro_rules! hex_id {
    ($t:ident, $n:expr) => {
        impl $t {
            pub const ZERO: $t = $t([0u8; $n]);

            pub fn is_zero(&self) -> bool {
                self.0 == [0u8; $n]
            }

            pub fn from_bytes(b: &[u8]) -> Option<Self> {
                if b.len() != $n {
                    return None;
                }
                let mut a = [0u8; $n];
                a.copy_from_slice(b);
                Some(Self(a))
            }

            pub fn from_hex(s: &str) -> Option<Self> {
                let v = hex::decode(s).ok()?;
                Self::from_bytes(&v)
            }

            pub fn to_hex(&self) -> String {
                hex::encode(self.0)
            }

            pub fn random() -> Self {
                let u = Uuid::new_v4();
                let mut a = [0u8; $n];
                a.copy_from_slice(&u.as_bytes()[..$n]);
                Self(a)
            }
        }

        impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($t), self.to_hex())
            }
        }

        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl From<$t> for String {
            fn from(v: $t) -> String {
                v.to_hex()
            }
        }

        impl TryFrom<String> for $t {
            type Error = String;
            fn try_from(s: String) -> Result<Self, Self::Error> {
                Self::from_hex(&s).ok_or_else(|| format!("invalid {}: {s}", stringify!($t)))
            }
        }
    };
}

hex_id!(TraceId, 16);
hex_id!(SpanId, 8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let t = TraceId::random();
        assert_eq!(TraceId::from_hex(&t.to_hex()), Some(t));
        let s = SpanId::random();
        assert_eq!(SpanId::from_hex(&s.to_hex()), Some(s));
        assert!(TraceId::from_hex("zz").is_none());
        assert!(SpanId::from_bytes(&[0; 4]).is_none());
    }

    #[test]
    fn serde_as_hex_string() {
        let t = TraceId([1u8; 16]);
        let j = serde_json::to_string(&t).unwrap();
        assert_eq!(j, "\"01010101010101010101010101010101\"");
        let back: TraceId = serde_json::from_str(&j).unwrap();
        assert_eq!(back, t);
    }
}
