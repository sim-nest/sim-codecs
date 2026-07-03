//! The canonical, lossless `Expr <-> JSON` projection.
//!
//! Maps each kernel `Expr` variant to and from a `$expr`-tagged
//! `serde_json::Value` so that every expression round-trips exactly.

use serde_json::{Value as JsonValue, json};
use sim_codec::DecodeBudget;
use sim_kernel::{Expr, NumberLiteral, Result};

use crate::helpers::{
    base64_decode, base64_encode, json_error, json_to_symbol, local_to_json, parse_quote_mode,
    parse_symbol, quote_mode_name, required_array, required_bool, required_str, required_value,
    symbol_from_json, symbol_to_json,
};

/// Projects an [`Expr`] onto a canonical `$expr`-tagged `serde_json::Value`.
///
/// This is the lossless encode half of the projection: every kernel `Expr`
/// variant maps to a tagged JSON form that [`json_to_expr`] reads back exactly.
pub fn expr_to_json(expr: &Expr) -> JsonValue {
    match expr {
        Expr::Nil => json!({ "$expr": "nil" }),
        Expr::Bool(value) => json!({ "$expr": "bool", "value": value }),
        Expr::Number(number) => json!({
            "$expr": "number",
            "domain": number.domain.to_string(),
            "value": number.canonical,
        }),
        Expr::Symbol(symbol) => symbol_to_json(symbol),
        Expr::Local(symbol) => local_to_json(symbol),
        Expr::String(value) => json!({ "$expr": "string", "value": value }),
        Expr::Bytes(value) => json!({
            "$expr": "bytes",
            "base64": base64_encode(value),
        }),
        Expr::List(items) => sequence_json("list", items),
        Expr::Vector(items) => sequence_json("vector", items),
        Expr::Map(entries) => {
            let mut sorted = entries.clone();
            sorted.sort_by_key(|(key, value)| (key.canonical_key(), value.canonical_key()));
            json!({
                "$expr": "map",
                "entries": sorted
                    .iter()
                    .map(|(key, value)| json!({
                        "key": expr_to_json(key),
                        "value": expr_to_json(value),
                    }))
                    .collect::<Vec<_>>(),
            })
        }
        Expr::Set(items) => {
            let mut sorted = items.clone();
            sorted.sort_by_key(Expr::canonical_key);
            sequence_json("set", &sorted)
        }
        Expr::Call { operator, args } => json!({
            "$expr": "call",
            "operator": expr_to_json(operator),
            "args": args.iter().map(expr_to_json).collect::<Vec<_>>(),
        }),
        Expr::Infix {
            operator,
            left,
            right,
        } => json!({
            "$expr": "infix",
            "operator": symbol_to_json(operator),
            "left": expr_to_json(left),
            "right": expr_to_json(right),
        }),
        Expr::Prefix { operator, arg } => json!({
            "$expr": "prefix",
            "operator": symbol_to_json(operator),
            "arg": expr_to_json(arg),
        }),
        Expr::Postfix { operator, arg } => json!({
            "$expr": "postfix",
            "operator": symbol_to_json(operator),
            "arg": expr_to_json(arg),
        }),
        Expr::Block(items) => sequence_json("block", items),
        Expr::Quote { mode, expr } => json!({
            "$expr": "quote",
            "mode": quote_mode_name(*mode),
            "expr": expr_to_json(expr),
        }),
        Expr::Annotated { expr, annotations } => json!({
            "$expr": "annotated",
            "expr": expr_to_json(expr),
            "annotations": annotations
                .iter()
                .map(|(key, value)| json!({
                    "key": symbol_to_json(key),
                    "value": expr_to_json(value),
                }))
                .collect::<Vec<_>>(),
        }),
        Expr::Extension { tag, payload } => json!({
            "$expr": "extension",
            "tag": symbol_to_json(tag),
            "payload": expr_to_json(payload),
        }),
    }
}

/// Reads a canonical `$expr`-tagged `serde_json::Value` back into an [`Expr`].
///
/// This is the lossless decode half of the projection, inverting
/// [`expr_to_json`]; it enforces the decode `budget` and node `depth` so
/// adversarial input cannot exhaust resources.
pub fn json_to_expr(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<Expr> {
    budget.enter_node(codec, depth)?;
    let Some(tag) = value.get("$expr").and_then(JsonValue::as_str) else {
        return Err(json_error(codec, "missing $expr"));
    };

    match tag {
        "nil" => Ok(Expr::Nil),
        "bool" => Ok(Expr::Bool(required_bool(codec, value, "value")?)),
        "number" => Ok(Expr::Number(NumberLiteral {
            domain: parse_symbol(required_str(codec, value, "domain")?),
            canonical: required_str(codec, value, "value")?.to_owned(),
        })),
        "symbol" => Ok(json_to_symbol(codec, value)?),
        "local" => Ok(Expr::Local(symbol_from_json(codec, value)?)),
        "string" => {
            let text = required_str(codec, value, "value")?;
            budget.check_string_bytes(codec, text.len())?;
            Ok(Expr::String(text.to_owned()))
        }
        "bytes" => {
            let encoded = required_str(codec, value, "base64")?;
            budget.check_blob_bytes(codec, encoded.len())?;
            let decoded = base64_decode(codec, encoded)?;
            budget.check_blob_bytes(codec, decoded.len())?;
            Ok(Expr::Bytes(decoded))
        }
        "list" => Ok(Expr::List(read_items(
            codec,
            value,
            "items",
            budget,
            depth + 1,
        )?)),
        "vector" => Ok(Expr::Vector(read_items(
            codec,
            value,
            "items",
            budget,
            depth + 1,
        )?)),
        "map" => {
            let entries = required_array(codec, value, "entries")?;
            budget.check_collection_len(codec, entries.len())?;
            Ok(Expr::Map(
                entries
                    .iter()
                    .map(|entry| {
                        Ok((
                            json_to_expr(
                                codec,
                                required_value(codec, entry, "key")?,
                                budget,
                                depth + 1,
                            )?,
                            json_to_expr(
                                codec,
                                required_value(codec, entry, "value")?,
                                budget,
                                depth + 1,
                            )?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?,
            ))
        }
        "set" => Ok(Expr::Set(read_items(
            codec,
            value,
            "items",
            budget,
            depth + 1,
        )?)),
        "call" => Ok(Expr::Call {
            operator: Box::new(json_to_expr(
                codec,
                required_value(codec, value, "operator")?,
                budget,
                depth + 1,
            )?),
            args: read_items(codec, value, "args", budget, depth + 1)?,
        }),
        "infix" => Ok(Expr::Infix {
            operator: symbol_from_json(codec, required_value(codec, value, "operator")?)?,
            left: Box::new(json_to_expr(
                codec,
                required_value(codec, value, "left")?,
                budget,
                depth + 1,
            )?),
            right: Box::new(json_to_expr(
                codec,
                required_value(codec, value, "right")?,
                budget,
                depth + 1,
            )?),
        }),
        "prefix" => Ok(Expr::Prefix {
            operator: symbol_from_json(codec, required_value(codec, value, "operator")?)?,
            arg: Box::new(json_to_expr(
                codec,
                required_value(codec, value, "arg")?,
                budget,
                depth + 1,
            )?),
        }),
        "postfix" => Ok(Expr::Postfix {
            operator: symbol_from_json(codec, required_value(codec, value, "operator")?)?,
            arg: Box::new(json_to_expr(
                codec,
                required_value(codec, value, "arg")?,
                budget,
                depth + 1,
            )?),
        }),
        "block" => Ok(Expr::Block(read_items(
            codec,
            value,
            "items",
            budget,
            depth + 1,
        )?)),
        "quote" => Ok(Expr::Quote {
            mode: parse_quote_mode(codec, required_str(codec, value, "mode")?)?,
            expr: Box::new(json_to_expr(
                codec,
                required_value(codec, value, "expr")?,
                budget,
                depth + 1,
            )?),
        }),
        "annotated" => {
            let annotations = required_array(codec, value, "annotations")?;
            budget.check_collection_len(codec, annotations.len())?;
            Ok(Expr::Annotated {
                expr: Box::new(json_to_expr(
                    codec,
                    required_value(codec, value, "expr")?,
                    budget,
                    depth + 1,
                )?),
                annotations: annotations
                    .iter()
                    .map(|entry| {
                        Ok((
                            symbol_from_json(codec, required_value(codec, entry, "key")?)?,
                            json_to_expr(
                                codec,
                                required_value(codec, entry, "value")?,
                                budget,
                                depth + 1,
                            )?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?,
            })
        }
        "extension" => Ok(Expr::Extension {
            tag: symbol_from_json(codec, required_value(codec, value, "tag")?)?,
            payload: Box::new(json_to_expr(
                codec,
                required_value(codec, value, "payload")?,
                budget,
                depth + 1,
            )?),
        }),
        other => Err(json_error(codec, format!("unknown expr tag {other}"))),
    }
}

fn sequence_json(tag: &'static str, items: &[Expr]) -> JsonValue {
    json!({
        "$expr": tag,
        "items": items.iter().map(expr_to_json).collect::<Vec<_>>(),
    })
}

fn read_items(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    field: &str,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<Vec<Expr>> {
    let items = required_array(codec, value, field)?;
    budget.check_collection_len(codec, items.len())?;
    items
        .iter()
        .map(|item| json_to_expr(codec, item, budget, depth))
        .collect()
}
