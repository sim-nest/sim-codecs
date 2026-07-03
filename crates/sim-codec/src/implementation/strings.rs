//! String-literal encoding and decoding.
//!
//! Renders a string to a quoted, escaped literal and parses such a literal
//! back, failing closed on malformed input.

use sim_kernel::{CodecId, Error, Result};

/// Render `value` as a double-quoted, backslash-escaped string literal.
///
/// Backslash, double quote, newline, carriage return, and tab use short
/// escapes; other control characters use the `\u{..}` form. Round-trips through
/// [`decode_string_literal`].
///
/// # Examples
///
/// ```
/// use sim_codec::{decode_string_literal, encode_string_literal};
/// use sim_kernel::CodecId;
///
/// let literal = encode_string_literal("a\tb\"c");
/// assert_eq!(literal, "\"a\\tb\\\"c\"");
/// assert_eq!(decode_string_literal(CodecId(0), &literal).unwrap(), "a\tb\"c");
/// ```
pub fn encode_string_literal(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write;
                write!(&mut out, "\\u{{{:x}}}", ch as u32).expect("string writes do not fail");
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Parse a double-quoted, escaped string literal back to its value.
///
/// Accepts the escapes produced by [`encode_string_literal`] and fails closed
/// with a codec error tagged `codec` on any malformed literal (missing quotes,
/// unterminated or unsupported escape, invalid Unicode scalar).
pub fn decode_string_literal(codec: CodecId, raw: &str) -> Result<String> {
    let inner = raw
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or_else(|| Error::CodecError {
            codec,
            message: format!("invalid string literal {raw}"),
        })?;
    let chars = inner.chars().collect::<Vec<_>>();
    let mut index = 0;
    let mut out = String::new();
    while index < chars.len() {
        let ch = chars[index];
        if ch != '\\' {
            out.push(ch);
            index += 1;
            continue;
        }

        index += 1;
        let escaped = *chars.get(index).ok_or_else(|| Error::CodecError {
            codec,
            message: format!("unterminated string escape in {raw}"),
        })?;
        match escaped {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                index += 1;
                if chars.get(index) != Some(&'{') {
                    return Err(Error::CodecError {
                        codec,
                        message: format!("invalid unicode escape in {raw}"),
                    });
                }
                let mut hex = String::new();
                index += 1;
                while let Some(ch) = chars.get(index) {
                    if *ch == '}' {
                        break;
                    }
                    hex.push(*ch);
                    index += 1;
                }
                if chars.get(index) != Some(&'}') || hex.is_empty() {
                    return Err(Error::CodecError {
                        codec,
                        message: format!("invalid unicode escape in {raw}"),
                    });
                }
                let scalar = u32::from_str_radix(&hex, 16).map_err(|_| Error::CodecError {
                    codec,
                    message: format!("invalid unicode escape in {raw}"),
                })?;
                let decoded = char::from_u32(scalar).ok_or_else(|| Error::CodecError {
                    codec,
                    message: format!("invalid unicode scalar in {raw}"),
                })?;
                out.push(decoded);
            }
            other => {
                return Err(Error::CodecError {
                    codec,
                    message: format!("unsupported string escape \\{other} in {raw}"),
                });
            }
        }
        index += 1;
    }
    Ok(out)
}
