//! Standard base64 encode/decode for the text framing layer.
//!
//! A small, dependency-free base64 codec (standard `+/` alphabet with padding)
//! used to wrap and unwrap binary frames as ASCII text.

use sim_kernel::{CodecId, Error, Result};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes `bytes` as standard base64 text (the `+/` alphabet, padded).
///
/// Shared with `sim-codec-bitwise-base64` so the two text wrappers use one
/// base64 implementation rather than forking the alphabet and logic.
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decodes standard base64 `text` back to bytes, failing closed on any invalid
/// character, length, or padding.
///
/// Shared with `sim-codec-bitwise-base64` (see [`encode_base64`]).
pub fn decode_base64(codec: CodecId, text: &str) -> Result<Vec<u8>> {
    let clean = strip_ascii_whitespace(codec, text)?;
    if !clean.len().is_multiple_of(4) {
        return codec_err(codec, "invalid base64 length");
    }

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    let chunk_count = clean.len() / 4;
    for (index, chunk) in clean.chunks_exact(4).enumerate() {
        if chunk[0] == b'=' || chunk[1] == b'=' {
            return codec_err(codec, "invalid base64 padding");
        }

        let is_last = index + 1 == chunk_count;
        let c0 = decode_b64(codec, chunk[0])?;
        let c1 = decode_b64(codec, chunk[1])?;
        let c2 = decode_optional_b64(codec, chunk[2])?;
        let c3 = decode_optional_b64(codec, chunk[3])?;

        match (c2, c3) {
            (Some(c2), Some(c3)) => {
                out.push((c0 << 2) | (c1 >> 4));
                out.push(((c1 & 0x0f) << 4) | (c2 >> 2));
                out.push(((c2 & 0x03) << 6) | c3);
            }
            (Some(c2), None) => {
                if !is_last {
                    return codec_err(codec, "base64 padding before final quartet");
                }
                if c2 & 0x03 != 0 {
                    return codec_err(codec, "non-zero base64 padding bits");
                }
                out.push((c0 << 2) | (c1 >> 4));
                out.push(((c1 & 0x0f) << 4) | (c2 >> 2));
            }
            (None, None) => {
                if !is_last {
                    return codec_err(codec, "base64 padding before final quartet");
                }
                if c1 & 0x0f != 0 {
                    return codec_err(codec, "non-zero base64 padding bits");
                }
                out.push((c0 << 2) | (c1 >> 4));
            }
            (None, Some(_)) => return codec_err(codec, "invalid base64 padding"),
        }
    }
    Ok(out)
}

fn strip_ascii_whitespace(codec: CodecId, text: &str) -> Result<Vec<u8>> {
    let mut clean = Vec::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b' ' | b'\n' | b'\r' | b'\t' => {}
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => clean.push(byte),
            _ => {
                return Err(Error::CodecError {
                    codec,
                    message: format!("invalid base64 byte 0x{byte:02x}"),
                });
            }
        }
    }
    Ok(clean)
}

fn decode_optional_b64(codec: CodecId, byte: u8) -> Result<Option<u8>> {
    if byte == b'=' {
        Ok(None)
    } else {
        decode_b64(codec, byte).map(Some)
    }
}

fn decode_b64(codec: CodecId, byte: u8) -> Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Error::CodecError {
            codec,
            message: format!("invalid base64 byte 0x{byte:02x}"),
        }),
    }
}

fn codec_err<T>(codec: CodecId, message: impl Into<String>) -> Result<T> {
    Err(Error::CodecError {
        codec,
        message: message.into(),
    })
}
