//! Signed minimal-magnitude integer conversion (no bigint dependency).
//!
//! The codec never hardwires a number tower: it inspects only the canonical
//! text of a `NumberLiteral`. A normalized decimal integer of any sign becomes
//! a sign bit plus its exact significant magnitude bits (converted by local
//! decimal divmod); everything else (floats, rationals, symbolic forms) falls
//! back to canonical text.

/// `Some(0..=15)` when `canonical` is a normalized non-negative integer in the
/// inline-literal range; `None` otherwise. Used to pick a `UInt*` tag.
pub(crate) fn small_uint_literal(canonical: &str) -> Option<u8> {
    if !is_normalized_unsigned(canonical) {
        return None;
    }
    match canonical.parse::<u16>() {
        Ok(value) if value < 16 => Some(value as u8),
        _ => None,
    }
}

/// `Some((negative, magnitude_bits))` when `canonical` is a normalized decimal
/// integer; `None` for non-integers (the caller falls back to canonical text).
///
/// The magnitude bits are most-significant-first with the top bit set (or empty
/// for zero), so they carry no leading zero bit.
pub(crate) fn integer_to_bits(canonical: &str) -> Option<(bool, Vec<bool>)> {
    let (negative, digits) = match canonical.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, canonical),
    };
    if !is_normalized_unsigned(digits) {
        return None;
    }
    if digits == "0" {
        // A normalized zero is never signed; "-0" is not a canonical form.
        return if negative {
            None
        } else {
            Some((false, Vec::new()))
        };
    }
    let mut decimal: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    let mut bits_lsb = Vec::new();
    while !decimal.iter().all(|&d| d == 0) {
        let remainder = divmod2(&mut decimal);
        bits_lsb.push(remainder == 1);
    }
    bits_lsb.reverse();
    Some((negative, bits_lsb))
}

/// Reverses [`integer_to_bits`]: rebuilds the canonical decimal string from a
/// sign flag and most-significant-first magnitude bits.
pub(crate) fn bits_to_integer(negative: bool, bits: &[bool]) -> String {
    // Decimal digits, most significant first; starts at zero.
    let mut decimal = vec![0u8];
    for &bit in bits {
        mul2_add(&mut decimal, u8::from(bit));
    }
    let text: String = decimal.iter().map(|&d| (d + b'0') as char).collect();
    if text == "0" {
        return text;
    }
    if negative { format!("-{text}") } else { text }
}

/// Whether `text` is a normalized non-negative decimal integer: `"0"`, or a
/// non-empty digit string with no leading zero.
fn is_normalized_unsigned(text: &str) -> bool {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    !(text.len() > 1 && text.starts_with('0'))
}

/// Divides the big-endian decimal digit vector by two in place, returning the
/// remainder (`0` or `1`).
fn divmod2(decimal: &mut [u8]) -> u8 {
    let mut carry = 0u8;
    for digit in decimal.iter_mut() {
        let current = carry * 10 + *digit;
        *digit = current / 2;
        carry = current % 2;
    }
    carry
}

/// Multiplies the big-endian decimal digit vector by two and adds `add` (0/1),
/// normalizing away a single leading zero introduced by the initial `[0]`.
fn mul2_add(decimal: &mut Vec<u8>, add: u8) {
    let mut carry = add;
    for digit in decimal.iter_mut().rev() {
        let current = *digit * 2 + carry;
        *digit = current % 10;
        carry = current / 10;
    }
    if carry > 0 {
        decimal.insert(0, carry);
    }
    while decimal.len() > 1 && decimal[0] == 0 {
        decimal.remove(0);
    }
}
