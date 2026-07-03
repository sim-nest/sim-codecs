use std::sync::Arc;

use sim_kernel::{
    DefaultFactory, EagerPolicy, Expr, LocatedExpr, LocatedExprTree, NumberLiteral, Origin,
    QuoteMode, SourceId, Span, Symbol, Trivia,
};

use crate::bitio::{BitReader, BitWriter, read_len, read_vbits, write_len, write_vbits};
use crate::number::{bits_to_integer, integer_to_bits, small_uint_literal};
use crate::types::BitwiseTag;
use crate::{
    BitwiseCodecLib, BitwiseFrame, DecodeLimits, canonical_bytes, decode_frame,
    decode_located_frame, decode_located_tree_frame, decode_located_tree_frame_with_limits,
    encode_dense, encode_frame, encode_located_frame, encode_located_tree_frame,
};

// ---- helpers --------------------------------------------------------------

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let lib = BitwiseCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

fn bit_length(value: u128) -> usize {
    (u128::BITS - value.leading_zeros()) as usize
}

fn gamma_bits(len: usize) -> usize {
    let m = len as u128 + 1;
    let k = (u128::BITS - m.leading_zeros()) as usize;
    2 * k - 1
}

fn reader(bytes: &[u8]) -> BitReader<'_> {
    BitReader::new(sim_kernel::CodecId(1), bytes, DecodeLimits::default()).unwrap()
}

fn num(domain: &str, canonical: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", domain),
        canonical: canonical.to_owned(),
    })
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

// ---- BITWISE2.03: plain Expr encode/decode + determinism ------------------

#[test]
fn expr_round_trip_scalars() {
    let cases = [
        Expr::Nil,
        Expr::Bool(true),
        Expr::Bool(false),
        Expr::Symbol(Symbol::qualified("math", "pi")),
        Expr::Local(Symbol::new("arg0")),
        Expr::String("line\n\"quoted\"".to_owned()),
        Expr::Bytes(vec![0, 1, 2, 0xff]),
    ];
    for expr in cases {
        let BitwiseFrame(bytes) = encode_frame(&expr).unwrap();
        let (_tables, decoded) = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap();
        assert!(decoded.canonical_eq(&expr), "round trip {expr:?}");
    }
}

#[test]
fn collections_cross_byte_boundaries() {
    let expr = Expr::List(vec![
        Expr::Nil,
        Expr::Bool(true),
        Expr::Vector(vec![Expr::Bool(false), num("i64", "7"), Expr::Nil]),
        Expr::Map(vec![
            (Expr::Symbol(Symbol::new("k")), Expr::Bool(true)),
            (Expr::Symbol(Symbol::new("j")), num("i64", "255")),
        ]),
        Expr::Set(vec![
            Expr::String("z".to_owned()),
            Expr::String("a".to_owned()),
        ]),
    ]);
    let BitwiseFrame(bytes) = encode_frame(&expr).unwrap();
    let (_tables, decoded) = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap();
    assert!(decoded.canonical_eq(&expr));
}

#[test]
fn full_expr_surface_round_trips() {
    let mut cx = cx();
    let expr = Expr::Annotated {
        expr: Box::new(Expr::Extension {
            tag: Symbol::qualified("demo", "wire"),
            payload: Box::new(Expr::Block(vec![
                Expr::Nil,
                Expr::Quote {
                    mode: QuoteMode::Syntax,
                    expr: Box::new(Expr::Infix {
                        operator: Symbol::new("+"),
                        left: Box::new(Expr::Prefix {
                            operator: Symbol::new("-"),
                            arg: Box::new(num("i64", "4")),
                        }),
                        right: Box::new(Expr::Postfix {
                            operator: Symbol::new("!"),
                            arg: Box::new(Expr::Symbol(Symbol::new("n"))),
                        }),
                    }),
                },
                Expr::Call {
                    operator: Box::new(Expr::Symbol(Symbol::qualified("math", "add"))),
                    args: vec![Expr::String("x".to_owned()), num("i64", "1000")],
                },
            ])),
        }),
        annotations: vec![(Symbol::new("count"), num("i64", "2"))],
    };
    let decoded = sim_test_support::roundtrip(&mut cx, "bitwise", &expr);
    assert!(decoded.canonical_eq(&expr));
}

#[test]
fn equal_exprs_encode_to_equal_bytes() {
    let left = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("b")), Expr::Bool(false)),
        (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
    ]);
    let right = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
        (Expr::Symbol(Symbol::new("b")), Expr::Bool(false)),
    ]);
    assert_eq!(encode_frame(&left).unwrap(), encode_frame(&right).unwrap());

    let left = Expr::Set(vec![
        Expr::String("z".to_owned()),
        Expr::String("a".to_owned()),
    ]);
    let right = Expr::Set(vec![
        Expr::String("a".to_owned()),
        Expr::String("z".to_owned()),
    ]);
    assert_eq!(encode_frame(&left).unwrap(), encode_frame(&right).unwrap());
}

// ---- BITWISE2.04: signed minimal-magnitude numbers ------------------------

#[test]
fn integer_255_is_eight_magnitude_bits() {
    let (negative, bits) = integer_to_bits("255").unwrap();
    assert!(!negative);
    assert_eq!(bits.len(), 8);
    assert!(bits.iter().all(|&b| b));
    assert_eq!(bits_to_integer(false, &bits), "255");
}

#[test]
fn negative_255_is_sign_plus_eight() {
    let (negative, bits) = integer_to_bits("-255").unwrap();
    assert!(negative);
    assert_eq!(bits.len(), 8);
    assert_eq!(bits_to_integer(true, &bits), "-255");
}

#[test]
fn zero_uses_uint0() {
    assert_eq!(small_uint_literal("0"), Some(0));
    assert_eq!(small_uint_literal("15"), Some(15));
    assert_eq!(small_uint_literal("16"), None);
    assert_eq!(small_uint_literal("-1"), None);
    // integer_to_bits("0") is an empty magnitude, decoding back to "0".
    let (negative, bits) = integer_to_bits("0").unwrap();
    assert!(!negative);
    assert!(bits.is_empty());
    assert_eq!(bits_to_integer(false, &bits), "0");
}

#[test]
fn non_integer_falls_back_to_text() {
    assert_eq!(integer_to_bits("1.5"), None);
    assert_eq!(integer_to_bits("1/3"), None);
    assert_eq!(integer_to_bits("01"), None); // non-normalized
    assert_eq!(integer_to_bits(""), None);
    for canonical in ["1.5", "1/3", "6.02e23"] {
        let expr = num("f64", canonical);
        let BitwiseFrame(bytes) = encode_frame(&expr).unwrap();
        let (_tables, decoded) = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap();
        assert!(decoded.canonical_eq(&expr), "text fallback {canonical}");
    }
}

#[test]
fn domains_round_trip() {
    for (domain, canonical) in [
        ("i64", "255"),
        ("i64", "-255"),
        ("i64", "0"),
        ("i64", "7"),
        ("bigint", "170141183460469231731687303715884105728"),
        ("rational", "3/4"),
        ("f64", "42.5"),
    ] {
        let expr = num(domain, canonical);
        let BitwiseFrame(bytes) = encode_frame(&expr).unwrap();
        let (_tables, decoded) = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap();
        assert!(
            decoded.canonical_eq(&expr),
            "domain {domain} value {canonical}"
        );
    }
}

#[test]
fn magnitude_bit_count_equals_bit_length() {
    for value in [
        1u128,
        2,
        3,
        8,
        16,
        255,
        256,
        1000,
        1_000_000,
        u64::MAX as u128,
    ] {
        let (_neg, bits) = integer_to_bits(&value.to_string()).unwrap();
        assert_eq!(bits.len(), bit_length(value), "magnitude bits for {value}");
        assert!(bits[0], "top magnitude bit must be 1 for {value}");
    }
}

// ---- BITWISE2.05: located and tree origin roles ---------------------------

#[test]
fn located_origin_round_trips_when_requested() {
    let located = LocatedExpr {
        expr: Expr::String("wire".to_owned()),
        origin: Some(Origin {
            codec: sim_kernel::CodecId(3),
            source: SourceId("cache.bit".to_owned()),
            span: Span { start: 10, end: 14 },
            trivia: vec![
                Trivia::Whitespace(" ".to_owned()),
                Trivia::BlockComment("/*x*/".to_owned()),
            ],
        }),
    };
    let BitwiseFrame(bytes) = encode_located_frame(&located, true).unwrap();
    let (_tables, decoded) = decode_located_frame(sim_kernel::CodecId(3), &bytes).unwrap();
    assert_eq!(decoded, located);
}

#[test]
fn tree_origin_round_trips() {
    let tree = LocatedExprTree {
        expr: Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("math", "add"))),
            args: vec![num("i64", "1"), num("i64", "2")],
        },
        origin: Some(Origin {
            codec: sim_kernel::CodecId(3),
            source: SourceId("tree.bit".to_owned()),
            span: Span { start: 0, end: 5 },
            trivia: Vec::new(),
        }),
        children: vec![
            LocatedExprTree::without_children(
                Expr::Symbol(Symbol::qualified("math", "add")),
                Some(Origin {
                    codec: sim_kernel::CodecId(3),
                    source: SourceId("tree.bit".to_owned()),
                    span: Span { start: 0, end: 1 },
                    trivia: Vec::new(),
                }),
            ),
            LocatedExprTree::without_children(num("i64", "1"), None),
            LocatedExprTree::without_children(num("i64", "2"), None),
        ],
    };
    let BitwiseFrame(bytes) = encode_located_tree_frame(&tree, true).unwrap();
    let (_tables, decoded) = decode_located_tree_frame(sim_kernel::CodecId(3), &bytes).unwrap();
    assert_eq!(decoded, tree);
}

#[test]
fn plain_encode_drops_origin() {
    let located = LocatedExpr {
        expr: Expr::String("wire".to_owned()),
        origin: Some(Origin {
            codec: sim_kernel::CodecId(3),
            source: SourceId("cache.bit".to_owned()),
            span: Span { start: 1, end: 2 },
            trivia: Vec::new(),
        }),
    };
    let with_flag = encode_located_frame(&located, false).unwrap();
    let plain = encode_frame(&located.expr).unwrap();
    assert_eq!(with_flag, plain, "plain encode must drop origin bytes");
    let (_tables, decoded) = decode_located_frame(sim_kernel::CodecId(3), &plain.0).unwrap();
    assert_eq!(decoded.origin, None);
}

#[test]
fn tree_encode_rejects_malformed_tree() {
    let tree = LocatedExprTree {
        expr: Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::new("f"))),
            args: vec![Expr::Bool(true)],
        },
        origin: None,
        children: vec![LocatedExprTree::without_children(
            Expr::Symbol(Symbol::new("f")),
            None,
        )],
    };
    assert!(encode_located_tree_frame(&tree, false).is_err());
}

// ---- BITWISE2.06: canonical content-addressing + matrix -------------------

#[test]
fn equal_values_share_canonical_bytes() {
    let left = Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("b")),
            Expr::Set(vec![
                Expr::String("y".to_owned()),
                Expr::String("x".to_owned()),
            ]),
        ),
        (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
    ]);
    let right = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("b")),
            Expr::Set(vec![
                Expr::String("x".to_owned()),
                Expr::String("y".to_owned()),
            ]),
        ),
    ]);
    assert_eq!(
        canonical_bytes(&left).unwrap(),
        canonical_bytes(&right).unwrap()
    );
}

#[test]
fn canonical_bytes_are_idempotent() {
    let expr = Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("math", "add"))),
        args: vec![num("i64", "-255"), num("f64", "1.5"), Expr::Nil],
    };
    let bytes = canonical_bytes(&expr).unwrap();
    let (_tables, decoded) = decode_frame(sim_kernel::CodecId(1), &bytes).unwrap();
    assert_eq!(canonical_bytes(&decoded).unwrap(), bytes);
}

#[test]
fn bitwise_smoke_round_trips_representative_expr() {
    // The shared cross-codec matrix now lives centrally in sim-sdk; this is a
    // minimal in-crate smoke test that codec:bitwise round-trips and re-encodes
    // a representative value stably.
    let mut cx = cx();
    let expr = Expr::List(vec![
        Expr::Nil,
        Expr::Bool(true),
        num("i64", "-255"),
        Expr::Map(vec![
            (Expr::Symbol(Symbol::new("b")), Expr::Bool(false)),
            (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
        ]),
    ]);
    let decoded = sim_test_support::roundtrip(&mut cx, "bitwise", &expr);
    assert!(decoded.canonical_eq(&expr));
    assert_eq!(
        canonical_bytes(&expr).unwrap(),
        canonical_bytes(&decoded).unwrap()
    );
}

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
