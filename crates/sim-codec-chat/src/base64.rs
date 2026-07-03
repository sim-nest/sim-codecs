//! Standalone base64 encode/decode for binary content parts in chat
//! transcripts, so the codec carries no external base64 dependency.

use sim_kernel::{CodecId, Error, Result};

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

pub(crate) fn base64_decode(codec: CodecId, raw: &str) -> Result<Vec<u8>> {
    let bytes = raw.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(codec_error(
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
            return Err(codec_error(codec, "invalid base64 padding"));
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
                    return Err(codec_error(codec, "base64 padding before final quartet"));
                }
                if c2 & 0x03 != 0 {
                    return Err(codec_error(codec, "non-zero base64 padding bits"));
                }
                out.push((c0 << 2) | (c1 >> 4));
                out.push(((c1 & 0x0f) << 4) | (c2 >> 2));
            }
            (None, None) => {
                if !is_last {
                    return Err(codec_error(codec, "base64 padding before final quartet"));
                }
                if c1 & 0x0f != 0 {
                    return Err(codec_error(codec, "non-zero base64 padding bits"));
                }
                out.push((c0 << 2) | (c1 >> 4));
            }
            (None, Some(_)) => return Err(codec_error(codec, "invalid base64 padding")),
        }
    }
    Ok(out)
}

fn decode_base64_char(codec: CodecId, ch: char) -> Result<u8> {
    match ch {
        'A'..='Z' => Ok((ch as u8) - b'A'),
        'a'..='z' => Ok((ch as u8) - b'a' + 26),
        '0'..='9' => Ok((ch as u8) - b'0' + 52),
        '+' => Ok(62),
        '/' => Ok(63),
        other => Err(codec_error(
            codec,
            format!("invalid base64 character {other}"),
        )),
    }
}

fn decode_base64_pad(codec: CodecId, ch: char) -> Result<Option<u8>> {
    if ch == '=' {
        Ok(None)
    } else {
        decode_base64_char(codec, ch).map(Some)
    }
}

fn codec_error(codec: CodecId, message: impl Into<String>) -> Error {
    Error::CodecError {
        codec,
        message: message.into(),
    }
}
