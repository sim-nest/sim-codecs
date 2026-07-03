//! Bit-granular I/O and the `vbits` minimal-integer primitive.
//!
//! [`BitWriter`] and [`BitReader`] pack fields across byte boundaries, so no
//! field is byte-aligned inside the logical frame. [`write_vbits`] /
//! [`read_vbits`] are the one size-of-size primitive used for every length,
//! table index, magnitude length, and span: they emit the value's significant
//! bit-length first (Elias-gamma-coded) and then exactly that many bits, so no
//! leading zero magnitude bit is ever written.

use sim_kernel::{Error, Result};

use crate::DecodeLimits;

/// A most-significant-bit-first bit sink backed by a growable byte buffer.
///
/// `write_bit` appends a fresh zero byte at each byte boundary and only ever
/// sets `1` bits, so the final byte is already zero-padded in its unused low
/// bits when [`BitWriter::finish`] hands the buffer back.
pub(crate) struct BitWriter {
    bytes: Vec<u8>,
    len_bits: usize,
}

impl BitWriter {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            len_bits: 0,
        }
    }

    pub(crate) fn write_bit(&mut self, bit: bool) {
        if self.len_bits.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if bit {
            let byte = self.len_bits / 8;
            self.bytes[byte] |= 1 << (7 - (self.len_bits % 8));
        }
        self.len_bits += 1;
    }

    /// Writes the low `width` bits of `value`, most significant first.
    pub(crate) fn write_bits(&mut self, value: u128, width: usize) {
        for offset in (0..width).rev() {
            self.write_bit((value >> offset) & 1 != 0);
        }
    }

    /// Writes each byte as eight bits, packing across the current bit cursor.
    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_bits(u128::from(byte), 8);
        }
    }

    /// The number of bits written so far (used by the bit-layout tests).
    #[cfg(test)]
    pub(crate) fn bit_len(&self) -> usize {
        self.len_bits
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// A most-significant-bit-first bit cursor over a borrowed byte frame.
///
/// Reads never advance past `end_bit`; every primitive fails closed on a short
/// frame, and [`BitReader::require_zero_padding`] enforces the self-delimiting
/// contract (no trailing whole byte, no nonzero trailing bit).
pub(crate) struct BitReader<'a> {
    codec: sim_kernel::CodecId,
    bytes: &'a [u8],
    bit_index: usize,
    end_bit: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(
        codec: sim_kernel::CodecId,
        bytes: &'a [u8],
        limits: DecodeLimits,
    ) -> Result<Self> {
        if bytes.len() > limits.max_frame_bytes {
            return Err(Error::CodecError {
                codec,
                message: format!(
                    "bitwise frame exceeds decode limit: {} > {} bytes",
                    bytes.len(),
                    limits.max_frame_bytes
                ),
            });
        }
        Ok(Self {
            codec,
            bytes,
            bit_index: 0,
            end_bit: bytes.len() * 8,
        })
    }

    pub(crate) fn read_bit(&mut self) -> Result<bool> {
        if self.bit_index >= self.end_bit {
            return Err(self.error("unexpected end of bitwise frame"));
        }
        let byte = self.bytes[self.bit_index / 8];
        let bit = (byte >> (7 - (self.bit_index % 8))) & 1 != 0;
        self.bit_index += 1;
        Ok(bit)
    }

    /// Reads `width` bits, most significant first, into a `u128`.
    pub(crate) fn read_bits(&mut self, width: usize) -> Result<u128> {
        let mut value = 0u128;
        for _ in 0..width {
            value = (value << 1) | u128::from(self.read_bit()?);
        }
        Ok(value)
    }

    /// Reads `count` whole bytes from the (possibly unaligned) bit cursor.
    pub(crate) fn read_bytes(&mut self, count: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            out.push(self.read_bits(8)? as u8);
        }
        Ok(out)
    }

    pub(crate) fn remaining(&self) -> usize {
        self.end_bit - self.bit_index
    }

    /// Enforces the self-delimiting frame contract: no trailing whole byte may
    /// remain, and every remaining bit of the final carrier byte must be zero.
    pub(crate) fn require_zero_padding(&mut self) -> Result<()> {
        if self.remaining() >= 8 {
            return Err(self.error("trailing bytes after bitwise payload"));
        }
        while self.bit_index < self.end_bit {
            if self.read_bit()? {
                return Err(self.error("nonzero trailing padding bit"));
            }
        }
        Ok(())
    }

    pub(crate) fn error(&self, message: impl Into<String>) -> Error {
        Error::CodecError {
            codec: self.codec,
            message: message.into(),
        }
    }
}

/// The significant bit-length of `value`; `0` when `value == 0`.
fn bit_length(value: u128) -> usize {
    (u128::BITS - value.leading_zeros()) as usize
}

/// Writes the Elias-gamma code of `value + 1` (`value` is a small non-negative
/// count, e.g. a bit-length). Self-delimiting, so the reader needs no width.
fn write_gamma(out: &mut BitWriter, value: usize) {
    let m = (value as u128) + 1;
    let k = bit_length(m);
    // k >= 1 because m >= 1; emit k-1 lead zeros then the k-bit value of m.
    for _ in 0..(k - 1) {
        out.write_bit(false);
    }
    out.write_bits(m, k);
}

/// Reads a value written by [`write_gamma`].
fn read_gamma(input: &mut BitReader<'_>) -> Result<usize> {
    let mut zeros = 0usize;
    while !input.read_bit()? {
        zeros += 1;
        if zeros >= 128 {
            return Err(input.error("bitwise gamma prefix too large"));
        }
    }
    // The stopping `1` bit is the MSB of m; read `zeros` more low bits.
    let rest = input.read_bits(zeros)?;
    let m = (1u128 << zeros) | rest;
    usize::try_from(m - 1).map_err(|_| input.error("bitwise gamma value overflows usize"))
}

/// Writes `value` with no leading zero magnitude bits: its bit-length first
/// (the "size of the size"), then exactly that many significant bits.
pub(crate) fn write_vbits(out: &mut BitWriter, value: u128) {
    let len = bit_length(value);
    write_gamma(out, len);
    out.write_bits(value, len);
}

/// Reads a value written by [`write_vbits`], rejecting a non-minimal encoding.
pub(crate) fn read_vbits(input: &mut BitReader<'_>) -> Result<u128> {
    let len = read_gamma(input)?;
    if len > 128 {
        return Err(input.error("bitwise vbits length overflows u128"));
    }
    let value = input.read_bits(len)?;
    if len > 0 && value >> (len - 1) == 0 {
        return Err(input.error("bitwise vbits carries a leading zero bit"));
    }
    Ok(value)
}

/// Writes a length or table index via [`write_vbits`]; intent reads at the call.
pub(crate) fn write_len(out: &mut BitWriter, len: usize) {
    write_vbits(out, len as u128);
}

/// Reads a length/index and checks it against `limit` before the caller allocates.
pub(crate) fn read_len(input: &mut BitReader<'_>, limit: usize) -> Result<usize> {
    let value = read_vbits(input)?;
    let len = usize::try_from(value).map_err(|_| input.error("length overflows usize"))?;
    if len > limit {
        return Err(input.error(format!("length exceeds decode limit: {len} > {limit}")));
    }
    Ok(len)
}
