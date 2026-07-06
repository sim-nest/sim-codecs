//! Plain `Expr` encode/decode, determinism, and signed minimal-magnitude
//! number tests (BITWISE2.03 and BITWISE2.04).

use sim_kernel::{Expr, QuoteMode, Symbol};

use crate::number::{bits_to_integer, integer_to_bits, small_uint_literal};
use crate::{BitwiseFrame, decode_frame, encode_frame};

use super::{bit_length, cx, num};

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
