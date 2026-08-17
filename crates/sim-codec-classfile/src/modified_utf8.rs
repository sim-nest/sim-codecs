//! Exact, strict JVM modified UTF-8 conversion.

use sim_text::CodeUnitString;

use crate::{ByteError, ByteErrorKind, ByteReader, ByteWriter};

/// Decode strict JVM modified UTF-8 into exact UTF-16 code units.
///
/// Literal zero, overlong forms other than the required two-byte NUL, four-byte
/// UTF-8, and unpaired surrogates are rejected. No scalar or lossy conversion occurs.
pub fn decode_modified_utf8(
    bytes: &[u8],
    code_unit_budget: usize,
) -> Result<CodeUnitString, ByteError> {
    let mut reader = ByteReader::new(bytes, code_unit_budget);
    let mut units = Vec::new();
    let mut pending_high: Option<usize> = None;
    while reader.remaining() != 0 {
        let at = reader.offset();
        let first = reader.read_u1()?;
        let unit = match first {
            0 => {
                return Err(ByteError::new(
                    ByteErrorKind::IllegalZero,
                    at,
                    "literal zero is illegal in modified UTF-8",
                ));
            }
            1..=0x7f => u16::from(first),
            0xc0..=0xdf => {
                let second = continuation(&mut reader)?;
                let value = (u16::from(first & 0x1f) << 6) | u16::from(second & 0x3f);
                if value == 0 {
                    if first != 0xc0 || second != 0x80 {
                        return Err(ByteError::new(
                            ByteErrorKind::OverlongModifiedUtf8,
                            at,
                            "non-canonical modified UTF-8 NUL",
                        ));
                    }
                } else if value < 0x80 {
                    return Err(ByteError::new(
                        ByteErrorKind::OverlongModifiedUtf8,
                        at,
                        "overlong two-byte modified UTF-8",
                    ));
                }
                value
            }
            0xe0..=0xef => {
                let second = continuation(&mut reader)?;
                let third = continuation(&mut reader)?;
                let value = (u16::from(first & 0x0f) << 12)
                    | (u16::from(second & 0x3f) << 6)
                    | u16::from(third & 0x3f);
                if value < 0x800 {
                    return Err(ByteError::new(
                        ByteErrorKind::OverlongModifiedUtf8,
                        at,
                        "overlong three-byte modified UTF-8",
                    ));
                }
                value
            }
            _ => {
                return Err(ByteError::new(
                    ByteErrorKind::InvalidModifiedUtf8,
                    at,
                    "invalid modified UTF-8 lead byte",
                ));
            }
        };

        if let Some(high_at) = pending_high.take() {
            if !(0xdc00..=0xdfff).contains(&unit) {
                return Err(ByteError::new(
                    ByteErrorKind::MalformedSurrogate,
                    high_at,
                    "high surrogate is not followed by a low surrogate",
                ));
            }
        } else if (0xd800..=0xdbff).contains(&unit) {
            pending_high = Some(at);
        } else if (0xdc00..=0xdfff).contains(&unit) {
            return Err(ByteError::new(
                ByteErrorKind::MalformedSurrogate,
                at,
                "low surrogate has no preceding high surrogate",
            ));
        }
        let next_len = units.len() + 1;
        if next_len > code_unit_budget {
            return Err(ByteError::new(
                ByteErrorKind::BudgetExceeded,
                at,
                format!("code-unit length {next_len} exceeds budget {code_unit_budget}"),
            ));
        }
        units.try_reserve_exact(1).map_err(|error| {
            ByteError::new(
                ByteErrorKind::BudgetExceeded,
                at,
                format!("code-unit allocation failed: {error}"),
            )
        })?;
        units.push(unit);
    }
    if let Some(at) = pending_high {
        return Err(ByteError::new(
            ByteErrorKind::MalformedSurrogate,
            at,
            "high surrogate is truncated",
        ));
    }
    Ok(CodeUnitString::try_from_code_units(units)
        .expect("allocation preflight enforces the tighter caller budget"))
}

fn continuation(reader: &mut ByteReader<'_>) -> Result<u8, ByteError> {
    let at = reader.offset();
    let byte = reader.read_u1()?;
    if byte & 0xc0 != 0x80 {
        return Err(ByteError::new(
            ByteErrorKind::InvalidModifiedUtf8,
            at,
            "invalid modified UTF-8 continuation byte",
        ));
    }
    Ok(byte)
}

/// Encode exact, well-formed UTF-16 code units as strict JVM modified UTF-8.
pub fn encode_modified_utf8(
    text: &CodeUnitString,
    byte_budget: usize,
) -> Result<Vec<u8>, ByteError> {
    let units = text.as_code_units();
    validate_surrogates(units)?;
    let required = units.iter().try_fold(0usize, |length, unit| {
        let width = if *unit == 0 {
            2
        } else if *unit <= 0x7f {
            1
        } else if *unit <= 0x7ff {
            2
        } else {
            3
        };
        length.checked_add(width).ok_or_else(|| {
            ByteError::new(
                ByteErrorKind::LengthOverflow,
                length,
                "modified UTF-8 output length overflow",
            )
        })
    })?;
    let mut writer = ByteWriter::new(byte_budget);
    // One checked reservation/write per unit keeps the hard budget authoritative.
    if required > byte_budget {
        return Err(ByteError::new(
            ByteErrorKind::BudgetExceeded,
            0,
            format!("modified UTF-8 output length {required} exceeds budget {byte_budget}"),
        ));
    }
    for unit in units {
        match *unit {
            0 => writer.write_bytes(&[0xc0, 0x80])?,
            1..=0x7f => writer.write_u1(*unit as u8)?,
            0x80..=0x7ff => {
                writer.write_bytes(&[0xc0 | (*unit >> 6) as u8, 0x80 | (*unit & 0x3f) as u8])?
            }
            _ => writer.write_bytes(&[
                0xe0 | (*unit >> 12) as u8,
                0x80 | ((*unit >> 6) & 0x3f) as u8,
                0x80 | (*unit & 0x3f) as u8,
            ])?,
        }
    }
    Ok(writer.into_bytes())
}

fn validate_surrogates(units: &[u16]) -> Result<(), ByteError> {
    let mut index = 0;
    while index < units.len() {
        match units[index] {
            0xd800..=0xdbff
                if units
                    .get(index + 1)
                    .is_some_and(|next| (0xdc00..=0xdfff).contains(next)) =>
            {
                index += 2
            }
            0xd800..=0xdfff => {
                return Err(ByteError::new(
                    ByteErrorKind::MalformedSurrogate,
                    index,
                    "unpaired surrogate code unit",
                ));
            }
            _ => index += 1,
        }
    }
    Ok(())
}
