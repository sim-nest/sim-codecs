//! Dense structural-sharing mode tests plus codec registration and
//! fail-closed decoding (BITWISE3.01 and the registration checks).

use sim_kernel::{Expr, Symbol};

use crate::bitio::{BitWriter, read_vbits, write_len, write_vbits};
use crate::types::BitwiseTag;
use crate::{canonical_bytes, decode_frame, encode_dense, encode_frame};

use super::{cx, num, reader};

// ---- BITWISE3.01: dense structural-sharing mode --------------------------

/// Reads the version and flags prefix of a frame, so tests can assert the plain
/// body never sets the dense flag (and therefore never emits a `Ref`).
fn frame_flags(bytes: &[u8]) -> u128 {
    let mut r = reader(bytes);
    let _version = read_vbits(&mut r).unwrap();
    read_vbits(&mut r).unwrap()
}

fn repeated_subtree_expr() -> Expr {
    let shared = Expr::List(vec![
        Expr::Symbol(Symbol::qualified("math", "add")),
        num("i64", "255"),
        Expr::String("a repeated payload".to_owned()),
    ]);
    Expr::List(vec![shared.clone(), shared.clone(), shared])
}

#[test]
fn dense_shares_repeated_subtrees_and_is_strictly_smaller() {
    let expr = repeated_subtree_expr();
    let plain = encode_frame(&expr).unwrap();
    let dense = encode_dense(&expr).unwrap();
    assert!(
        dense.0.len() < plain.0.len(),
        "dense must be strictly smaller: {} vs {}",
        dense.0.len(),
        plain.0.len()
    );
    let (_tables, decoded) = decode_frame(sim_kernel::CodecId(1), &dense.0).unwrap();
    assert!(decoded.canonical_eq(&expr), "dense round trip {decoded:?}");
}

#[test]
fn default_output_sets_no_dense_flag() {
    // A plain frame never sets the dense flag, so it can never contain a Ref
    // (the reader only takes the Ref branch when the dense flag is set); the
    // dense encoder does set it.
    let expr = repeated_subtree_expr();
    assert_eq!(frame_flags(&encode_frame(&expr).unwrap().0) & 4, 0);
    assert_eq!(frame_flags(&canonical_bytes(&expr).unwrap()) & 4, 0);
    assert_eq!(frame_flags(&encode_dense(&expr).unwrap().0) & 4, 4);
}

#[test]
fn canonical_bytes_stay_plain_and_ref_free() {
    // canonical_bytes is the plain encode_frame output regardless of repetition,
    // and dense is a strictly smaller, separate serialization.
    let expr = repeated_subtree_expr();
    assert_eq!(
        canonical_bytes(&expr).unwrap(),
        encode_frame(&expr).unwrap().0
    );
    assert!(encode_dense(&expr).unwrap().0.len() < canonical_bytes(&expr).unwrap().len());
}

#[test]
fn dense_decode_rejects_out_of_range_ref() {
    let mut w = BitWriter::new();
    write_vbits(&mut w, 1); // version
    write_vbits(&mut w, 4); // FLAG_DENSE
    write_len(&mut w, 0); // libs
    write_len(&mut w, 0); // symbols
    write_len(&mut w, 0); // number domains
    // body: a one-element List whose child is Ref(5) -- out of range.
    w.write_bits(BitwiseTag::List as u128, BitwiseTag::WIDTH_BITS);
    write_len(&mut w, 1);
    w.write_bits(BitwiseTag::Ref as u128, BitwiseTag::WIDTH_BITS);
    write_vbits(&mut w, 5);
    let bytes = w.finish();
    let err = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(message.contains("out of range"), "{message}")
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn dense_decode_rejects_forward_ref() {
    let mut w = BitWriter::new();
    write_vbits(&mut w, 1); // version
    write_vbits(&mut w, 4); // FLAG_DENSE
    write_len(&mut w, 0); // libs
    write_len(&mut w, 0); // symbols
    write_len(&mut w, 0); // number domains
    // body: a one-element List whose child is Ref(0) -- points at the still
    // in-progress List itself (a forward/self reference).
    w.write_bits(BitwiseTag::List as u128, BitwiseTag::WIDTH_BITS);
    write_len(&mut w, 1);
    w.write_bits(BitwiseTag::Ref as u128, BitwiseTag::WIDTH_BITS);
    write_vbits(&mut w, 0);
    let bytes = w.finish();
    let err = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(message.contains("forward reference"), "{message}")
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn plain_frame_rejects_ref_tag() {
    // Without the dense flag a Ref tag in the body is not accepted.
    let mut w = BitWriter::new();
    write_vbits(&mut w, 1); // version
    write_vbits(&mut w, 0); // no flags
    write_len(&mut w, 0); // libs
    write_len(&mut w, 0); // symbols
    write_len(&mut w, 0); // number domains
    w.write_bits(BitwiseTag::Ref as u128, BitwiseTag::WIDTH_BITS);
    write_vbits(&mut w, 0);
    let bytes = w.finish();
    assert!(decode_frame(sim_kernel::CodecId(1), &bytes).is_err());
}

// ---- registration + fail-closed -------------------------------------------

#[test]
fn codec_registers() {
    let cx = cx();
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "bitwise"))
            .is_some()
    );
}

#[test]
fn malformed_frame_fails_closed() {
    assert!(decode_frame(sim_kernel::CodecId(9), b"\xff\xff\xff\xff").is_err());
}
