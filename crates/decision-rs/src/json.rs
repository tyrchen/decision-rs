//! Bounded JSON parsing with duplicate-key rejection, and bounded serialization.

use std::{
    cell::Cell,
    collections::HashSet,
    fmt,
    io::{self, Write},
};

use serde::{
    Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};

use crate::error::Error;

pub(crate) const MAX_DEPTH: usize = 32;
pub(crate) const MAX_NODES: usize = 10_000;

struct Budget {
    nodes: Cell<usize>,
    max_nodes: usize,
    max_depth: usize,
    string_limit: usize,
    key_limit: usize,
}
struct Seed<'a> {
    budget: &'a Budget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        let nodes = self.budget.nodes.get().saturating_add(1);
        if nodes > self.budget.max_nodes || self.depth > self.budget.max_depth {
            return Err(de::Error::custom("JSON resource limit"));
        }
        self.budget.nodes.set(nodes);
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        if v.len() > self.budget.string_limit {
            return Err(E::custom("string limit"));
        }
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        if v.len() > self.budget.string_limit {
            return Err(E::custom("string limit"));
        }
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = a.next_element_seed(Seed {
            budget: self.budget,
            depth: self.depth.saturating_add(1),
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        let mut keys = HashSet::new();
        while let Some(key) = a.next_key::<String>()? {
            if key.len() > self.budget.key_limit || !keys.insert(key.clone()) {
                return Err(de::Error::custom("duplicate or overlong key"));
            }
            let value = a.next_value_seed(Seed {
                budget: self.budget,
                depth: self.depth.saturating_add(1),
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

pub(crate) fn parse(bytes: &[u8], string_limit: usize, key_limit: usize) -> Result<Value, Error> {
    parse_bounded(bytes, string_limit, key_limit, MAX_NODES, MAX_DEPTH)
}
pub(crate) fn parse_file(bytes: &[u8]) -> Result<Value, Error> {
    parse_bounded(bytes, 1 << 20, 256, 1_000_000, 40)
}
fn parse_bounded(
    bytes: &[u8],
    string_limit: usize,
    key_limit: usize,
    max_nodes: usize,
    max_depth: usize,
) -> Result<Value, Error> {
    let budget = Budget {
        nodes: Cell::new(0),
        string_limit,
        key_limit,
        max_nodes,
        max_depth,
    };
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let value = Seed {
        budget: &budget,
        depth: 0,
    }
    .deserialize(&mut d)
    .map_err(|_| Error::protocol("malformed, duplicate, or excessive JSON"))?;
    d.end().map_err(|_| Error::protocol("trailing JSON data"))?;
    Ok(value)
}

/// Checks size and nesting while serde writes, before recursive allocation.
struct CappedWriter {
    bytes: Vec<u8>,
    cap: usize,
    depth: usize,
    quoted: bool,
    escaped: bool,
}
impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.cap {
            return Err(io::Error::other("serialized size limit"));
        }
        for b in bytes {
            if self.quoted {
                if self.escaped {
                    self.escaped = false;
                } else if *b == b'\\' {
                    self.escaped = true;
                } else if *b == b'"' {
                    self.quoted = false;
                }
            } else {
                match b {
                    b'"' => self.quoted = true,
                    b'{' | b'[' => {
                        self.depth = self.depth.saturating_add(1);
                        if self.depth > MAX_DEPTH {
                            return Err(io::Error::other("depth limit"));
                        }
                    }
                    b'}' | b']' => self.depth = self.depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn encode<T: Serialize>(value: &T, cap: usize) -> Result<Vec<u8>, Error> {
    let mut writer = CappedWriter {
        bytes: Vec::new(),
        cap,
        depth: 0,
        quoted: false,
        escaped: false,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| Error::InvalidInput("serialization or resource limit"))?;
    Ok(writer.bytes)
}
