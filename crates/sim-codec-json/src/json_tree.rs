//! Codec-owned JSON data model and bounded text boundary.

use serde_json::{Map, Number, Value};
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{CodecId, Error, Result};

/// A dependency-neutral JSON tree.
///
/// This is the public interchange model for guests that need JSON data without
/// adopting the codec's parser implementation as part of their own API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonTree {
    /// The JSON `null` value.
    Null,
    /// A JSON boolean.
    Bool(bool),
    /// A JSON number in its validated textual representation.
    Number(String),
    /// A JSON string.
    String(String),
    /// A JSON array.
    Array(Vec<JsonTree>),
    /// A JSON object.
    Object(Vec<(String, JsonTree)>),
}

/// Parses JSON text into the codec-owned tree under default decode limits.
pub fn parse_json(codec: CodecId, source: &str) -> Result<JsonTree> {
    parse_json_with_limits(codec, source, DecodeLimits::default())
}

/// Parses JSON text into the codec-owned tree under explicit decode limits.
pub fn parse_json_with_limits(
    codec: CodecId,
    source: &str,
    limits: DecodeLimits,
) -> Result<JsonTree> {
    let mut budget = DecodeBudget::new(limits);
    budget.check_input_bytes(codec, source.len())?;
    let value = serde_json::from_str(source).map_err(|error| json_error(codec, error))?;
    JsonTree::from_value(codec, value, &mut budget, 0)
}

/// Renders a codec-owned JSON tree as compact canonical text.
pub fn render_json(codec: CodecId, tree: &JsonTree) -> Result<String> {
    serde_json::to_string(&tree.to_value(codec)?).map_err(|error| json_error(codec, error))
}

impl JsonTree {
    pub(crate) fn from_json_value(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(value),
            Value::Number(value) => Self::Number(value.to_string()),
            Value::String(value) => Self::String(value),
            Value::Array(values) => {
                Self::Array(values.into_iter().map(Self::from_json_value).collect())
            }
            Value::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from_json_value(value)))
                    .collect(),
            ),
        }
    }

    pub(crate) fn to_json_value(&self, codec: CodecId) -> Result<Value> {
        self.to_value(codec)
    }

    fn from_value(
        codec: CodecId,
        value: Value,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<Self> {
        budget.enter_node(codec, depth)?;
        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Bool(value)),
            Value::Number(value) => Ok(Self::Number(value.to_string())),
            Value::String(value) => {
                budget.check_string_bytes(codec, value.len())?;
                Ok(Self::String(value))
            }
            Value::Array(values) => {
                budget.check_collection_len(codec, values.len())?;
                values
                    .into_iter()
                    .map(|value| Self::from_value(codec, value, budget, depth + 1))
                    .collect::<Result<Vec<_>>>()
                    .map(Self::Array)
            }
            Value::Object(values) => {
                budget.check_collection_len(codec, values.len())?;
                values
                    .into_iter()
                    .map(|(key, value)| {
                        budget.check_string_bytes(codec, key.len())?;
                        Ok((key, Self::from_value(codec, value, budget, depth + 1)?))
                    })
                    .collect::<Result<Vec<_>>>()
                    .map(Self::Object)
            }
        }
    }

    fn to_value(&self, codec: CodecId) -> Result<Value> {
        match self {
            Self::Null => Ok(Value::Null),
            Self::Bool(value) => Ok(Value::Bool(*value)),
            Self::Number(value) => value
                .parse::<Number>()
                .map(Value::Number)
                .map_err(|error| json_error(codec, error)),
            Self::String(value) => Ok(Value::String(value.clone())),
            Self::Array(values) => values
                .iter()
                .map(|value| value.to_value(codec))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array),
            Self::Object(values) => values
                .iter()
                .map(|(key, value)| Ok((key.clone(), value.to_value(codec)?)))
                .collect::<Result<Map<_, _>>>()
                .map(Value::Object),
        }
    }
}

fn json_error(codec: CodecId, error: impl std::fmt::Display) -> Error {
    Error::CodecError {
        codec,
        message: error.to_string(),
    }
}
