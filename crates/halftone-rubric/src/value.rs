//! The JSON value the interpreter works on: like `serde_json::Value`, but objects
//! keep their keys in insertion order, as Python dicts do. The reference evaluator
//! walks assertions in file order (ingredient order, pseudo-node numbering), so
//! order is part of the result. Numbers keep Python's int/float distinction.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// A JSON value with ordered objects and Python-style int/float numbers.
#[derive(Debug, Clone, Default)]
pub enum JValue {
    /// `null` (Python `None`).
    #[default]
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// An integer (Python `int`).
    Int(i128),
    /// A floating-point number (Python `float`).
    Float(f64),
    /// A string.
    Str(String),
    /// An array.
    Array(Vec<JValue>),
    /// An object, in insertion order.
    Object(Map),
}

/// An object's members in insertion order. Re-inserting a key replaces its value
/// in place, as a Python dict does.
#[derive(Debug, Clone, Default)]
pub struct Map(Vec<(String, JValue)>);

impl Map {
    /// An empty object.
    pub fn new() -> Self {
        Map(Vec::new())
    }
    /// The value at `key`.
    pub fn get(&self, key: &str) -> Option<&JValue> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    /// Whether `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }
    /// Set `key`, keeping its position if it already exists.
    pub fn insert(&mut self, key: impl Into<String>, value: JValue) {
        let key = key.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }
    /// Members in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &JValue)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    /// Number of members.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether the object has no members.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl JValue {
    /// Parse JSON text, keeping object key order.
    pub fn from_json_str(s: &str) -> Result<JValue, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// The member `key` of an object, or `None` for anything else.
    pub fn get(&self, key: &str) -> Option<&JValue> {
        match self {
            JValue::Object(m) => m.get(key),
            _ => None,
        }
    }

    /// The string content, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Whether this is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, JValue::Null)
    }

    /// Python truthiness (`bool(x)`): `None`, `False`, zero, `""`, `[]` and `{}` are
    /// false.
    pub fn py_truthy(&self) -> bool {
        match self {
            JValue::Null => false,
            JValue::Bool(b) => *b,
            JValue::Int(i) => *i != 0,
            JValue::Float(f) => *f != 0.0,
            JValue::Str(s) => !s.is_empty(),
            JValue::Array(a) => !a.is_empty(),
            JValue::Object(m) => !m.is_empty(),
        }
    }

    /// Convert to a `serde_json::Value` (object order is then up to serde_json).
    pub fn to_serde(&self) -> serde_json::Value {
        use serde_json::Value as V;
        match self {
            JValue::Null => V::Null,
            JValue::Bool(b) => V::Bool(*b),
            JValue::Int(i) => {
                if let Ok(v) = i64::try_from(*i) {
                    V::from(v)
                } else if let Ok(v) = u64::try_from(*i) {
                    V::from(v)
                } else {
                    V::from(*i as f64)
                }
            }
            JValue::Float(f) => serde_json::Number::from_f64(*f).map_or(V::Null, V::Number),
            JValue::Str(s) => V::String(s.clone()),
            JValue::Array(a) => V::Array(a.iter().map(JValue::to_serde).collect()),
            JValue::Object(m) => {
                V::Object(m.iter().map(|(k, v)| (k.clone(), v.to_serde())).collect())
            }
        }
    }

    /// Convert from a `serde_json::Value`, in whatever key order it holds.
    pub fn from_serde(v: &serde_json::Value) -> JValue {
        use serde_json::Value as V;
        match v {
            V::Null => JValue::Null,
            V::Bool(b) => JValue::Bool(*b),
            V::Number(n) => {
                if let Some(i) = n.as_i64() {
                    JValue::Int(i128::from(i))
                } else if let Some(u) = n.as_u64() {
                    JValue::Int(i128::from(u))
                } else {
                    JValue::Float(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            V::String(s) => JValue::Str(s.clone()),
            V::Array(a) => JValue::Array(a.iter().map(JValue::from_serde).collect()),
            V::Object(o) => {
                let mut m = Map::new();
                for (k, v) in o {
                    m.insert(k.clone(), JValue::from_serde(v));
                }
                JValue::Object(m)
            }
        }
    }
}

/// Python `==`: numbers compare by value across bool/int/float (`True == 1 == 1.0`),
/// containers element by element, dicts regardless of key order.
pub fn py_eq(a: &JValue, b: &JValue) -> bool {
    use JValue::*;
    fn num(v: &JValue) -> Option<f64> {
        match v {
            Bool(b) => Some(f64::from(u8::from(*b))),
            Int(i) => Some(*i as f64),
            Float(f) => Some(*f),
            _ => None,
        }
    }
    match (a, b) {
        (Null, Null) => true,
        (Int(x), Int(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| py_eq(p, q)),
        (Object(x), Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| py_eq(v, w)))
        }
        _ => match (num(a), num(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
    }
}

/// The Python type of a value, for `type(a) is type(b)`.
fn py_type(v: &JValue) -> u8 {
    match v {
        JValue::Null => 0,
        JValue::Bool(_) => 1,
        JValue::Int(_) => 2,
        JValue::Float(_) => 3,
        JValue::Str(_) => 4,
        JValue::Array(_) => 5,
        JValue::Object(_) => 6,
    }
}

/// json-formula's `strictDeepEqual`: the same top-level type and Python-equal.
/// (So `1 == 1.0` is false, but `[1] == [1.0]` is true, as in the reference.)
pub fn strict_deep_equal(a: &JValue, b: &JValue) -> bool {
    py_type(a) == py_type(b) && py_eq(a, b)
}

impl<'de> Deserialize<'de> for JValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = JValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_unit<E>(self) -> Result<JValue, E> {
                Ok(JValue::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<JValue, E> {
                Ok(JValue::Bool(b))
            }
            fn visit_i64<E>(self, i: i64) -> Result<JValue, E> {
                Ok(JValue::Int(i128::from(i)))
            }
            fn visit_u64<E>(self, u: u64) -> Result<JValue, E> {
                Ok(JValue::Int(i128::from(u)))
            }
            fn visit_f64<E>(self, f: f64) -> Result<JValue, E> {
                Ok(JValue::Float(f))
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<JValue, E> {
                Ok(JValue::Str(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> Result<JValue, E> {
                Ok(JValue::Str(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<JValue, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = seq.next_element()? {
                    v.push(x);
                }
                Ok(JValue::Array(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JValue, A::Error> {
                let mut m = Map::new();
                while let Some((k, v)) = map.next_entry::<String, JValue>()? {
                    m.insert(k, v);
                }
                Ok(JValue::Object(m))
            }
        }
        d.deserialize_any(V)
    }
}
