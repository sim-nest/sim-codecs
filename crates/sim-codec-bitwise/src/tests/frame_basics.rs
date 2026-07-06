//! Bit IO, vbits, tags, and header tests (BITWISE2.01 and BITWISE2.02).

use crate::bitio::{BitWriter, read_len, read_vbits, write_len, write_vbits};
use crate::types::BitwiseTag;
use crate::{DecodeLimits, decode_frame, decode_located_tree_frame_with_limits};

use super::{bit_length, reader};

fn gamma_bits(len: usize) -> usize {
    let m = len as u128 + 1;
    let k = (u128::BITS - m.leading_zeros()) as usize;
    2 * k - 1
}

// ---- BITWISE2.01: bit IO and vbits ----------------------------------------

#[test]
fn crosses_byte_boundaries() {
    let mut w = BitWriter::new();
    w.write_bits(0b10, 2);
    w.write_bits(0b011, 3);
    w.write_bits(0b11111, 5); // spills from byte 0 into byte 1
    assert_eq!(w.bit_len(), 10);
    let bytes = w.finish();
    assert_eq!(bytes.len(), 2, "10 bits must occupy two carrier bytes");

    let mut r = reader(&bytes);
    assert_eq!(r.read_bits(2).unwrap(), 0b10);
    assert_eq!(r.read_bits(3).unwrap(), 0b011);
    assert_eq!(r.read_bits(5).unwrap(), 0b11111);
}

#[test]
fn vbits_round_trip() {
    for value in [
        0u128,
        1,
        2,
        3,
        15,
        16,
        255,
        256,
        65_535,
        1 << 100,
        u128::MAX,
    ] {
        let mut w = BitWriter::new();
        write_vbits(&mut w, value);
        let bytes = w.finish();
        let mut r = reader(&bytes);
        assert_eq!(
            read_vbits(&mut r).unwrap(),
            value,
            "vbits round trip {value}"
        );
    }
}

#[test]
fn vbits_has_no_leading_zero_payload() {
    // The payload segment is exactly bit_length(value) bits, so no leading zero
    // magnitude bit is ever emitted; vbits(0) is a single bit.
    for value in [0u128, 1, 2, 7, 8, 255, 256, 1_000_000, u128::MAX] {
        let mut w = BitWriter::new();
        write_vbits(&mut w, value);
        let expected = gamma_bits(bit_length(value)) + bit_length(value);
        assert_eq!(
            w.bit_len(),
            expected,
            "vbits({value}) must carry exactly bit_length payload bits"
        );
    }
    let mut w = BitWriter::new();
    write_vbits(&mut w, 0);
    assert_eq!(w.bit_len(), 1, "vbits(0) is a single bit");
}

#[test]
fn read_vbits_rejects_non_minimal_encoding() {
    // Manually craft gamma(len=3) then payload 0b011 (top bit 0 -> non-minimal).
    let mut w = BitWriter::new();
    // gamma of len+1 = 4 -> k=3 -> "00" + "100"
    w.write_bit(false);
    w.write_bit(false);
    w.write_bits(0b100, 3);
    w.write_bits(0b011, 3); // 3-bit payload with a leading zero
    let bytes = w.finish();
    let mut r = reader(&bytes);
    assert!(read_vbits(&mut r).is_err());
}

#[test]
fn read_len_rejects_over_limit() {
    let mut w = BitWriter::new();
    write_len(&mut w, 100);
    let bytes = w.finish();
    let mut r = reader(&bytes);
    assert!(read_len(&mut r, 10).is_err());
}

#[test]
fn padding_must_be_zero() {
    // A stray 1 bit in the final carrier byte is rejected.
    let bytes = [0b0010_0000u8];
    let mut r = reader(&bytes);
    r.read_bits(2).unwrap();
    assert!(r.require_zero_padding().is_err());

    // All-zero remaining bits are accepted.
    let bytes = [0b1100_0000u8];
    let mut r = reader(&bytes);
    r.read_bits(2).unwrap();
    assert!(r.require_zero_padding().is_ok());

    // A whole trailing byte -- even if zero -- is rejected (non-canonical).
    let bytes = [0b1100_0000u8, 0x00];
    let mut r = reader(&bytes);
    r.read_bits(2).unwrap();
    assert!(r.require_zero_padding().is_err());
}

// ---- BITWISE2.02: tags and header -----------------------------------------

#[test]
fn every_defined_tag_round_trips() {
    for raw in 0u8..=36 {
        let tag = BitwiseTag::from_u6(raw).expect("defined tag");
        let mut w = BitWriter::new();
        w.write_bits(tag as u128, BitwiseTag::WIDTH_BITS);
        let bytes = w.finish();
        let mut r = reader(&bytes);
        let decoded = r.read_bits(BitwiseTag::WIDTH_BITS).unwrap() as u8;
        assert_eq!(BitwiseTag::from_u6(decoded), Some(tag));
    }
}

#[test]
fn reserved_tags_are_rejected() {
    for raw in 37u8..=63 {
        assert_eq!(BitwiseTag::from_u6(raw), None, "raw {raw} must be reserved");
    }
}

#[test]
fn decode_rejects_reserved_body_tag() {
    let mut w = BitWriter::new();
    write_vbits(&mut w, 1); // version
    write_vbits(&mut w, 0); // flags
    write_len(&mut w, 0); // libs
    write_len(&mut w, 0); // symbols
    write_len(&mut w, 0); // number domains
    w.write_bits(37, BitwiseTag::WIDTH_BITS); // reserved tag
    let bytes = w.finish();
    let err = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => assert!(message.contains("reserved")),
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn header_rejects_bad_version_flags_and_oversize() {
    // Unknown version.
    let mut w = BitWriter::new();
    write_vbits(&mut w, 2);
    let bytes = w.finish();
    assert!(decode_frame(sim_kernel::CodecId(1), &bytes).is_err());

    // Unknown flag bit (bit 3, value 8, is reserved and rejected; the dense bit
    // value 4 is now a known flag and handled by the dense-mode tests).
    let mut w = BitWriter::new();
    write_vbits(&mut w, 1);
    write_vbits(&mut w, 8);
    let bytes = w.finish();
    assert!(decode_frame(sim_kernel::CodecId(1), &bytes).is_err());

    // Oversize table under a tight limit.
    let mut w = BitWriter::new();
    write_vbits(&mut w, 1);
    write_vbits(&mut w, 0);
    write_len(&mut w, 5); // libs count
    let bytes = w.finish();
    let err = decode_located_tree_frame_with_limits(
        sim_kernel::CodecId(1),
        &bytes,
        DecodeLimits {
            max_table_entries: 2,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(message.contains("exceeds decode limit"))
        }
        other => panic!("unexpected error {other:?}"),
    }
}
