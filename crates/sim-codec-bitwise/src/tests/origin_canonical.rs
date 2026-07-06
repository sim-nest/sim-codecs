//! Located/tree origin roles and canonical content-addressing tests
//! (BITWISE2.05 and BITWISE2.06).

use sim_kernel::{Expr, LocatedExpr, LocatedExprTree, Origin, SourceId, Span, Symbol, Trivia};

use crate::{
    BitwiseFrame, canonical_bytes, decode_frame, decode_located_frame, decode_located_tree_frame,
    encode_frame, encode_located_frame, encode_located_tree_frame,
};

use super::{cx, num};

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
