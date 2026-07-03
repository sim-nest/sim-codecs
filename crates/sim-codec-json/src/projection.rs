//! Mode-aware Expr <-> JSON projections.
//!
//! `sim-codec-json` offers more than one way to map the kernel `Expr` graph
//! onto `serde_json::Value`. The canonical, lossless projection lives in
//! [`crate::expr_json`] and uses `$expr` tags so that every `Expr` round-trips
//! exactly. Some interop surfaces (notably third-party LLM tool schemas) need a
//! plain, untagged JSON shape instead. [`JsonProjectionMode`] selects which
//! projection to apply.

use serde_json::{Map, Value as JsonValue};
use sim_codec::DecodeBudget;
use sim_kernel::{CodecId, Expr, NumberLiteral, Result, Symbol};

use crate::expr_json;

/// Selects which `Expr <-> JSON` projection to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonProjectionMode {
    /// The canonical, lossless projection: `$expr`-tagged forms that round-trip
    /// every `Expr` exactly. Delegates to [`crate::expr_to_json`] /
    /// [`crate::json_to_expr`].
    TaggedCanonical,
    /// A lossy interop projection that emits/accepts plain, untagged JSON.
    ///
    /// This is intended for foreign surfaces (for example LLM tool schemas and
    /// tool-call arguments) that expect ordinary JSON without kernel tags. It is
    /// NOT the canonical codec: structure such as symbols vs strings, number
    /// domains, vectors vs lists, and most non-collection forms is collapsed and
    /// cannot be recovered. Use [`JsonProjectionMode::TaggedCanonical`] whenever
    /// faithful round-tripping is required.
    UntaggedInterop,
    /// A one-way, lossy text projection: `Expr -> JSON` produces a JSON string
    /// containing the debug rendering of the expression. There is no faithful
    /// inverse; `JSON -> Expr` reuses the [`JsonProjectionMode::UntaggedInterop`]
    /// decoder.
    TextLossy,
}

/// Reads a JSON number as `u64`, accepting an unsigned literal directly or a
/// non-negative signed literal. This is the one home for the provider
/// token-count parser that the OpenAI server, chat codec, and HTTP runner each
/// re-grew.
pub fn json_number_to_u64(value: &JsonValue) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
}

/// Projects an `Expr` to a `serde_json::Value` using the given mode.
pub fn project_expr_to_json(expr: &Expr, mode: JsonProjectionMode) -> JsonValue {
    match mode {
        JsonProjectionMode::TaggedCanonical => expr_json::expr_to_json(expr),
        JsonProjectionMode::UntaggedInterop => untagged_expr_to_json(expr),
        JsonProjectionMode::TextLossy => JsonValue::String(format!("{expr:?}")),
    }
}

/// Projects a `serde_json::Value` to an `Expr` using the given mode.
///
/// [`JsonProjectionMode::TaggedCanonical`] requires `$expr`-tagged input and is
/// fallible at the codec layer; this infallible entry point is only defined for
/// the lossy projections. For tagged canonical decoding use
/// [`crate::json_to_expr`] directly (it threads a decode budget and returns a
/// `Result`).
///
/// Both [`JsonProjectionMode::UntaggedInterop`] and
/// [`JsonProjectionMode::TextLossy`] decode plain JSON via the untagged
/// projection.
pub fn project_json_to_expr(value: &JsonValue, mode: JsonProjectionMode) -> Expr {
    match mode {
        // All infallible decode modes share the untagged decoder. Tagged
        // canonical decoding is fallible and lives behind `crate::json_to_expr`.
        JsonProjectionMode::TaggedCanonical
        | JsonProjectionMode::UntaggedInterop
        | JsonProjectionMode::TextLossy => untagged_json_to_expr(value),
    }
}

/// Budget-aware variant of [`project_json_to_expr`].
///
/// Decodes through the untagged projection like [`project_json_to_expr`] but
/// charges every produced node, collection length, and string length against
/// `budget`, so a hostile provider response cannot exhaust host resources. The
/// `mode` is accepted for signature parity; every decode mode shares the
/// untagged decoder.
pub fn project_json_to_expr_budgeted(
    value: &JsonValue,
    _mode: JsonProjectionMode,
    codec: CodecId,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<Expr> {
    budget.enter_node(codec, depth)?;
    match value {
        JsonValue::Null => Ok(Expr::Nil),
        JsonValue::Bool(value) => Ok(Expr::Bool(*value)),
        JsonValue::Number(number) => Ok(Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: number.to_string(),
        })),
        JsonValue::String(value) => {
            budget.check_string_bytes(codec, value.len())?;
            Ok(Expr::String(value.clone()))
        }
        JsonValue::Array(items) => {
            budget.check_collection_len(codec, items.len())?;
            let items = items
                .iter()
                .map(|item| {
                    project_json_to_expr_budgeted(
                        item,
                        JsonProjectionMode::UntaggedInterop,
                        codec,
                        budget,
                        depth + 1,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Expr::List(items))
        }
        JsonValue::Object(entries) => {
            budget.check_collection_len(codec, entries.len())?;
            let entries = entries
                .iter()
                .map(|(key, value)| {
                    Ok((
                        Expr::Symbol(Symbol::new(key.clone())),
                        project_json_to_expr_budgeted(
                            value,
                            JsonProjectionMode::UntaggedInterop,
                            codec,
                            budget,
                            depth + 1,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Expr::Map(entries))
        }
    }
}

// The bodies below produce the OpenAI server projection
// byte-for-byte. Do not "improve" them: foreign tool-schema golden tests depend
// on this exact untagged shape.

fn untagged_expr_to_json(expr: &Expr) -> JsonValue {
    match expr {
        Expr::Nil => JsonValue::Null,
        Expr::Bool(value) => JsonValue::Bool(*value),
        Expr::Number(number) => serde_json::from_str(&number.canonical)
            .unwrap_or_else(|_| JsonValue::String(number.canonical.clone())),
        Expr::String(value) => JsonValue::String(value.clone()),
        Expr::Symbol(symbol) | Expr::Local(symbol) => JsonValue::String(symbol.to_string()),
        Expr::List(items) | Expr::Vector(items) => {
            JsonValue::Array(items.iter().map(untagged_expr_to_json).collect())
        }
        Expr::Map(entries) => {
            let mut object = Map::new();
            for (key, value) in entries {
                object.insert(untagged_expr_key(key), untagged_expr_to_json(value));
            }
            JsonValue::Object(object)
        }
        other => JsonValue::String(format!("{other:?}")),
    }
}

fn untagged_expr_key(expr: &Expr) -> String {
    match expr {
        Expr::String(value) => value.clone(),
        Expr::Symbol(symbol) | Expr::Local(symbol) => symbol.as_qualified_str(),
        other => format!("{other:?}"),
    }
}

fn untagged_json_to_expr(value: &JsonValue) -> Expr {
    match value {
        JsonValue::Null => Expr::Nil,
        JsonValue::Bool(value) => Expr::Bool(*value),
        JsonValue::Number(number) => Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: number.to_string(),
        }),
        JsonValue::String(value) => Expr::String(value.clone()),
        JsonValue::Array(items) => Expr::List(items.iter().map(untagged_json_to_expr).collect()),
        JsonValue::Object(entries) => Expr::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    (
                        Expr::Symbol(Symbol::new(key.clone())),
                        untagged_json_to_expr(value),
                    )
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untagged_roundtrips_plain_object() {
        let expr = Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("name")),
                Expr::String("ada".to_owned()),
            ),
            (Expr::Symbol(Symbol::new("flag")), Expr::Bool(true)),
        ]);
        let json = project_expr_to_json(&expr, JsonProjectionMode::UntaggedInterop);
        assert_eq!(json["name"], JsonValue::String("ada".to_owned()));
        assert_eq!(json["flag"], JsonValue::Bool(true));
    }

    #[test]
    fn text_lossy_is_debug_string() {
        let expr = Expr::Bool(true);
        let json = project_expr_to_json(&expr, JsonProjectionMode::TextLossy);
        assert_eq!(json, JsonValue::String(format!("{expr:?}")));
    }
}
