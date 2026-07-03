//! Shared helpers for the JSON projections.
//!
//! Symbol/quote-mode conversion and typed field accessors (required/optional
//! string, bool, array, and base64 bytes) used by the canonical and tree
//! projections.

use serde_json::{Map, Value as JsonValue};
use sim_kernel::{Error, Expr, QuoteMode, Result, Symbol};

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
/// accepted and parsed with [`parse_symbol`] for backward compatibility with
/// the legacy `to_string()`-flattened encoding.
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
/// characters round-trip exactly. A namespace-less local encodes identically to
/// the legacy `{ "$expr": "local", "name": ... }` form.
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

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        let b0 = bytes[index];
        let b1 = bytes.get(index + 1).copied().unwrap_or(0);
        let b2 = bytes.get(index + 2).copied().unwrap_or(0);
        let word = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(ALPHABET[((word >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((word >> 12) & 0x3f) as usize] as char);
        if index + 1 < bytes.len() {
            out.push(ALPHABET[((word >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if index + 2 < bytes.len() {
            out.push(ALPHABET[(word & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        index += 3;
    }
    out
}

pub(crate) fn base64_decode(codec: sim_kernel::CodecId, raw: &str) -> Result<Vec<u8>> {
    let bytes = raw.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(json_error(
            codec,
            "base64 payload length must be a multiple of 4",
        ));
    }

    let chunk_count = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunk_count * 3);
    for (chunk_index, chunk) in bytes.chunks_exact(4).enumerate() {
        // Padding may only appear in the trailing one or two bytes of the final
        // quartet. Reject `=` in the first two positions, interior padding, and
        // pad-then-non-pad so non-canonical strings ("AA=A", "AA==AAAA") fail
        // closed instead of silently decoding.
        if chunk[0] == b'=' || chunk[1] == b'=' {
            return Err(json_error(codec, "invalid base64 padding"));
        }
        let is_last = chunk_index + 1 == chunk_count;
        let c0 = decode_base64_char(codec, chunk[0] as char)?;
        let c1 = decode_base64_char(codec, chunk[1] as char)?;
        let c2 = decode_base64_pad(codec, chunk[2] as char)?;
        let c3 = decode_base64_pad(codec, chunk[3] as char)?;
        match (c2, c3) {
            (Some(c2), Some(c3)) => {
                out.push((c0 << 2) | (c1 >> 4));
                out.push(((c1 & 0x0f) << 4) | (c2 >> 2));
                out.push(((c2 & 0x03) << 6) | c3);
            }
            (Some(c2), None) => {
                if !is_last {
                    return Err(json_error(codec, "base64 padding before final quartet"));
                }
                if c2 & 0x03 != 0 {
                    return Err(json_error(codec, "non-zero base64 padding bits"));
                }
                out.push((c0 << 2) | (c1 >> 4));
                out.push(((c1 & 0x0f) << 4) | (c2 >> 2));
            }
            (None, None) => {
                if !is_last {
                    return Err(json_error(codec, "base64 padding before final quartet"));
                }
                if c1 & 0x0f != 0 {
                    return Err(json_error(codec, "non-zero base64 padding bits"));
                }
                out.push((c0 << 2) | (c1 >> 4));
            }
            (None, Some(_)) => return Err(json_error(codec, "invalid base64 padding")),
        }
    }
    Ok(out)
}

fn decode_base64_char(codec: sim_kernel::CodecId, ch: char) -> Result<u8> {
    match ch {
        'A'..='Z' => Ok((ch as u8) - b'A'),
        'a'..='z' => Ok((ch as u8) - b'a' + 26),
        '0'..='9' => Ok((ch as u8) - b'0' + 52),
        '+' => Ok(62),
        '/' => Ok(63),
        other => Err(json_error(
            codec,
            format!("invalid base64 character {other}"),
        )),
    }
}

fn decode_base64_pad(codec: sim_kernel::CodecId, ch: char) -> Result<Option<u8>> {
    if ch == '=' {
        Ok(None)
    } else {
        decode_base64_char(codec, ch).map(Some)
    }
}
