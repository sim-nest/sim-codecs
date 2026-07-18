//! Shared helpers for the JSON projections.
//!
//! Symbol/quote-mode conversion and typed field accessors (required/optional
//! string, bool, array, and base64 bytes) used by the canonical and tree
//! projections.

use std::fmt::Write as _;

use serde_json::{Map, Value as JsonValue};
use sim_kernel::{Error, Expr, QuoteMode, Result, Symbol};

pub(crate) use sim_codec_binary_base64::{
    decode_base64 as base64_decode, encode_base64 as base64_encode,
};

/// Escapes a string for use inside a JSON string literal.
///
/// The returned text does not include the surrounding quote characters.
pub fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{00}'..='\u{1f}' => {
                write!(&mut out, "\\u{:04x}", ch as u32).expect("string writes do not fail");
            }
            other => out.push(other),
        }
    }
    out
}

pub(crate) fn symbol_to_json(symbol: &Symbol) -> JsonValue {
    let mut object = Map::new();
    object.insert("$expr".to_owned(), JsonValue::String("symbol".to_owned()));
    if let Some(namespace) = &symbol.namespace {
        object.insert(
            "namespace".to_owned(),
            JsonValue::String(namespace.to_string()),
        );
    }
    object.insert(
        "name".to_owned(),
        JsonValue::String(symbol.name.to_string()),
    );
    JsonValue::Object(object)
}

pub(crate) fn json_to_symbol(codec: sim_kernel::CodecId, value: &JsonValue) -> Result<Expr> {
    Ok(Expr::Symbol(symbol_from_json(codec, value)?))
}

/// Decodes a symbol carried in operator/tag/key/local position.
///
/// Accepts the structured `{ "namespace": ..., "name": ... }` form (the same
/// shape [`symbol_to_json`] emits) so that symbols containing `/` -- the
/// division operator, for one -- round-trip exactly. A bare JSON string is also
/// accepted and parsed with [`parse_symbol`] for compatibility with flattened
/// symbol encodings.
pub(crate) fn symbol_from_json(codec: sim_kernel::CodecId, value: &JsonValue) -> Result<Symbol> {
    if let Some(text) = value.as_str() {
        return Ok(parse_symbol(text));
    }
    let name = required_str(codec, value, "name")?;
    let namespace = optional_str(codec, value, "namespace")?;
    Ok(match namespace {
        Some(namespace) => Symbol::qualified(namespace.to_owned(), name.to_owned()),
        None => Symbol::new(name.to_owned()),
    })
}

/// Encodes an [`Expr::Local`] symbol as a `$expr: "local"` object that carries
/// the namespace and name as structured fields, so locals named with reserved
/// characters round-trip exactly. A namespace-less local encodes as
/// `{ "$expr": "local", "name": ... }`.
pub(crate) fn local_to_json(symbol: &Symbol) -> JsonValue {
    let mut object = Map::new();
    object.insert("$expr".to_owned(), JsonValue::String("local".to_owned()));
    if let Some(namespace) = &symbol.namespace {
        object.insert(
            "namespace".to_owned(),
            JsonValue::String(namespace.to_string()),
        );
    }
    object.insert(
        "name".to_owned(),
        JsonValue::String(symbol.name.to_string()),
    );
    JsonValue::Object(object)
}

pub(crate) fn quote_mode_name(mode: QuoteMode) -> &'static str {
    match mode {
        QuoteMode::Quote => "quote",
        QuoteMode::QuasiQuote => "quasiquote",
        QuoteMode::Unquote => "unquote",
        QuoteMode::Splice => "splice",
        QuoteMode::Syntax => "syntax",
    }
}

pub(crate) fn parse_quote_mode(codec: sim_kernel::CodecId, raw: &str) -> Result<QuoteMode> {
    match raw {
        "quote" => Ok(QuoteMode::Quote),
        "quasiquote" => Ok(QuoteMode::QuasiQuote),
        "unquote" => Ok(QuoteMode::Unquote),
        "splice" => Ok(QuoteMode::Splice),
        "syntax" => Ok(QuoteMode::Syntax),
        other => Err(json_error(codec, format!("unknown quote mode {other}"))),
    }
}

pub(crate) fn required_value<'a>(
    codec: sim_kernel::CodecId,
    value: &'a JsonValue,
    field: &str,
) -> Result<&'a JsonValue> {
    value
        .get(field)
        .ok_or_else(|| json_error(codec, format!("missing field {field}")))
}

pub(crate) fn required_str<'a>(
    codec: sim_kernel::CodecId,
    value: &'a JsonValue,
    field: &str,
) -> Result<&'a str> {
    required_value(codec, value, field)?
        .as_str()
        .ok_or_else(|| json_error(codec, format!("field {field} must be a string")))
}

pub(crate) fn optional_str<'a>(
    codec: sim_kernel::CodecId,
    value: &'a JsonValue,
    field: &str,
) -> Result<Option<&'a str>> {
    match value.get(field) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(other) => other
            .as_str()
            .map(Some)
            .ok_or_else(|| json_error(codec, format!("field {field} must be a string"))),
    }
}

pub(crate) fn required_bool(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    field: &str,
) -> Result<bool> {
    required_value(codec, value, field)?
        .as_bool()
        .ok_or_else(|| json_error(codec, format!("field {field} must be a bool")))
}

pub(crate) fn required_u64(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    field: &str,
) -> Result<u64> {
    required_value(codec, value, field)?
        .as_u64()
        .ok_or_else(|| json_error(codec, format!("field {field} must be a u64")))
}

pub(crate) fn required_array<'a>(
    codec: sim_kernel::CodecId,
    value: &'a JsonValue,
    field: &str,
) -> Result<&'a Vec<JsonValue>> {
    required_value(codec, value, field)?
        .as_array()
        .ok_or_else(|| json_error(codec, format!("field {field} must be an array")))
}

pub(crate) fn optional_bytes(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    field: &str,
) -> Result<Option<Vec<u8>>> {
    match value.get(field) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(other) => Ok(Some(
            serde_json::from_value::<Vec<u8>>(other.clone())
                .map_err(|err| json_error(codec, err.to_string()))?,
        )),
    }
}

pub(crate) fn parse_symbol(raw: &str) -> Symbol {
    match raw.split_once('/') {
        Some((namespace, name)) => Symbol::qualified(namespace.to_owned(), name.to_owned()),
        None => Symbol::new(raw.to_owned()),
    }
}

pub(crate) fn json_error(codec: sim_kernel::CodecId, message: impl Into<String>) -> Error {
    Error::CodecError {
        codec,
        message: message.into(),
    }
}
