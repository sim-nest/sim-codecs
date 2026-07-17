//! Duplicate-preserving checks for MCP-owned JSON wire object fields.

use std::{collections::BTreeSet, fmt};

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use sim_kernel::{CodecId, Result};

use crate::error::codec_error;

pub(crate) fn reject_duplicate_mcp_wire_keys(codec: CodecId, source: &str) -> Result<()> {
    let mut deserializer = serde_json::Deserializer::from_str(source);
    McpWireDuplicateCheck::deserialize(&mut deserializer)
        .map(|_| ())
        .map_err(|err| codec_error(codec, format!("MCP JSON parse error: {err}")))
}

struct McpWireDuplicateCheck;

impl<'de> Deserialize<'de> for McpWireDuplicateCheck {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(McpWireDuplicateVisitor)
    }
}

struct McpWireDuplicateVisitor;

impl<'de> Visitor<'de> for McpWireDuplicateVisitor {
    type Value = McpWireDuplicateCheck;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any MCP JSON-RPC value")
    }

    fn visit_bool<E>(self, _value: bool) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_i64<E>(self, _value: i64) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_u64<E>(self, _value: u64) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_f64<E>(self, _value: f64) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_str<E>(self, _value: &str) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_string<E>(self, _value: String) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(McpWireDuplicateCheck)
    }

    fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(McpWireDuplicateCheck)
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut seen = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!(
                    "duplicate MCP JSON-RPC field {key}"
                )));
            }
            if key == "error" {
                map.next_value::<McpErrorDuplicateCheck>()?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(McpWireDuplicateCheck)
    }
}

struct McpErrorDuplicateCheck;

impl<'de> Deserialize<'de> for McpErrorDuplicateCheck {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(McpErrorDuplicateVisitor)
    }
}

struct McpErrorDuplicateVisitor;

impl<'de> Visitor<'de> for McpErrorDuplicateVisitor {
    type Value = McpErrorDuplicateCheck;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any MCP error value")
    }

    fn visit_bool<E>(self, _value: bool) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_i64<E>(self, _value: i64) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_u64<E>(self, _value: u64) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_f64<E>(self, _value: f64) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_str<E>(self, _value: &str) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_string<E>(self, _value: String) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(McpErrorDuplicateCheck)
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut seen = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!(
                    "duplicate MCP error field {key}"
                )));
            }
            map.next_value::<IgnoredAny>()?;
        }
        Ok(McpErrorDuplicateCheck)
    }
}
