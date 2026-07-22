use std::sync::Arc;

// conformance: wire protocol grammar preserves binary frames.

use sim_codec::{
    DecodePosition, DecodedForm, Input, Output, decode_datum_with_codec, decode_default_with_codec,
    encode_datum_with_codec,
};
use sim_kernel::{
    Args, Datum, DefaultFactory, EagerPolicy, EncodeOptions, Expr, LocatedExpr, LocatedExprTree,
    NumberLiteral, Origin, QuoteMode, SourceId, Span, Symbol, Trivia,
};

use crate::{
    BinaryCodecLib, BinaryFrame, DecodeLimits, decode_frame, decode_located_frame,
    decode_located_tree_frame, decode_located_tree_frame_with_limits, encode_frame,
    encode_located_frame, encode_located_tree_frame,
};

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let lib = BinaryCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

#[test]
fn codec_registers() {
    let cx = cx();
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "binary"))
            .is_some()
    );
    assert!(
        cx.registry()
            .function_by_symbol(&Symbol::qualified("binary", "roundtrip-report"))
            .is_some()
    );
}

#[test]
fn roundtrip_report_function_runs() {
    let mut cx = cx();
    let report = call_report(&mut cx, Symbol::qualified("binary", "roundtrip-report"));
    assert_eq!(field_bool(&report, "roundtrip"), Some(true));
    assert_eq!(field_string(&report, "codec"), Some("codec/binary"));
}

#[test]
fn frame_header_carries_tables() {
    let expr = Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("math", "add"))),
        args: vec![Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1".to_owned(),
        })],
    };

    let BinaryFrame(frame) = encode_frame(&expr).unwrap();
    let (tables, decoded) = decode_frame(sim_kernel::CodecId(1), &frame).unwrap();

    assert_eq!(decoded, expr);
    assert_eq!(tables.libs, vec!["math".to_owned(), "numbers".to_owned()]);
    assert!(tables.symbols.contains(&Symbol::qualified("math", "add")));
    assert_eq!(
        tables.number_domains,
        vec![Symbol::qualified("numbers", "f64")]
    );
}

fn call_report(cx: &mut sim_kernel::Cx, symbol: Symbol) -> Expr {
    let value = cx.registry().function_by_symbol(&symbol).unwrap().clone();
    let callable = value.object().as_callable().unwrap();
    let value = callable.call(cx, Args::new(Vec::new())).unwrap();
    value.object().as_expr(cx).unwrap()
}

fn field_bool(expr: &Expr, name: &str) -> Option<bool> {
    map_field(expr, name).and_then(|value| match value {
        Expr::Bool(value) => Some(*value),
        _ => None,
    })
}

fn field_string<'a>(expr: &'a Expr, name: &str) -> Option<&'a str> {
    map_field(expr, name).and_then(|value| match value {
        Expr::String(value) => Some(value.as_str()),
        _ => None,
    })
}

fn map_field<'a>(expr: &'a Expr, name: &str) -> Option<&'a Expr> {
    let Expr::Map(entries) = expr else {
        return None;
    };
    entries.iter().find_map(|(key, value)| match key {
        Expr::Symbol(symbol) if symbol.name.as_ref() == name => Some(value),
        _ => None,
    })
}

#[test]
fn frame_is_canonical_for_map_and_set() {
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

#[test]
fn full_expr_surface_roundtrips() {
    let mut cx = cx();
    let expr = Expr::Annotated {
        expr: Box::new(Expr::Extension {
            tag: Symbol::qualified("demo", "wire"),
            payload: Box::new(Expr::Block(vec![
                Expr::Nil,
                Expr::Vector(vec![
                    Expr::Bool(true),
                    Expr::Bytes(vec![1, 2, 3]),
                    Expr::Quote {
                        mode: QuoteMode::Syntax,
                        expr: Box::new(Expr::Infix {
                            operator: Symbol::new("+"),
                            left: Box::new(Expr::Prefix {
                                operator: Symbol::new("-"),
                                arg: Box::new(Expr::Number(NumberLiteral {
                                    domain: Symbol::qualified("numbers", "f64"),
                                    canonical: "4".to_owned(),
                                })),
                            }),
                            right: Box::new(Expr::Postfix {
                                operator: Symbol::new("!"),
                                arg: Box::new(Expr::Symbol(Symbol::new("n"))),
                            }),
                        }),
                    },
                ]),
                Expr::Call {
                    operator: Box::new(Expr::Symbol(Symbol::qualified("math", "add"))),
                    args: vec![
                        Expr::String("x".to_owned()),
                        Expr::Map(vec![(
                            Expr::Symbol(Symbol::new("k")),
                            Expr::Set(vec![
                                Expr::String("a".to_owned()),
                                Expr::String("b".to_owned()),
                            ]),
                        )]),
                    ],
                },
            ])),
        }),
        annotations: vec![
            (
                Symbol::qualified("meta", "origin"),
                Expr::String("test".to_owned()),
            ),
            (
                Symbol::new("count"),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                }),
            ),
        ],
    };

    let decoded = sim_test_support::roundtrip(&mut cx, "binary", &expr);
    assert!(decoded.canonical_eq(&expr));
}

#[test]
fn datum_roundtrip_preserves_content_id() {
    let mut cx = cx();
    let datum = sample_datum();
    let content_id = datum.content_id().unwrap();

    let output = encode_datum_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "binary"),
        &datum,
        EncodeOptions::default(),
    )
    .unwrap();
    let input = match output {
        Output::Bytes(bytes) => Input::Bytes(bytes),
        Output::Text(text) => Input::Text(text),
    };
    let decoded = decode_datum_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "binary"),
        input,
        Default::default(),
    )
    .unwrap();

    assert_eq!(decoded, datum);
    assert_eq!(decoded.content_id().unwrap(), content_id);
}

#[test]
fn default_decode_returns_datum_even_in_eval_position() {
    let mut cx = cx();
    let datum = sample_datum();
    let output = encode_datum_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "binary"),
        &datum,
        EncodeOptions::default(),
    )
    .unwrap();
    let input = match output {
        Output::Bytes(bytes) => Input::Bytes(bytes),
        Output::Text(text) => Input::Text(text),
    };

    let decoded = decode_default_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "binary"),
        input,
        Default::default(),
        DecodePosition::Eval,
    )
    .unwrap();

    assert_eq!(decoded, DecodedForm::Datum(datum));
}

#[test]
fn malformed_frames_fail() {
    let err = decode_frame(sim_kernel::CodecId(9), b"BAD!").unwrap_err();
    match err {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, sim_kernel::CodecId(9));
            assert!(message.contains("magic mismatch"));
        }
        other => panic!("unexpected error {other:?}"),
    }
}

fn sample_datum() -> Datum {
    Datum::Node {
        tag: Symbol::qualified("demo", "binary"),
        fields: vec![
            (Symbol::new("name"), Datum::String("frame".to_owned())),
            (
                Symbol::new("payload"),
                Datum::Set(vec![
                    Datum::String("a".to_owned()),
                    Datum::String("b".to_owned()),
                ]),
            ),
        ],
    }
}

#[test]
fn located_frame_roundtrips_with_origin() {
    let located = LocatedExpr {
        expr: Expr::String("wire".to_owned()),
        origin: Some(Origin {
            codec: sim_kernel::CodecId(3),
            source: SourceId("cache.bin".to_owned()),
            span: Span { start: 10, end: 14 },
            trivia: vec![
                Trivia::Whitespace(" ".to_owned()),
                Trivia::BlockComment("/*x*/".to_owned()),
            ],
        }),
    };

    let BinaryFrame(bytes) = encode_located_frame(&located, true).unwrap();
    let (_tables, decoded) = decode_located_frame(sim_kernel::CodecId(3), &bytes).unwrap();
    assert_eq!(decoded, located);
}

#[test]
fn tree_frame_roundtrips_nested_origins() {
    let tree = LocatedExprTree {
        expr: Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("math", "add"))),
            args: vec![
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "1".to_owned(),
                }),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                }),
            ],
        },
        origin: Some(Origin {
            codec: sim_kernel::CodecId(3),
            source: SourceId("tree.bin".to_owned()),
            span: Span { start: 0, end: 5 },
            trivia: Vec::new(),
        }),
        children: vec![
            LocatedExprTree::without_children(
                Expr::Symbol(Symbol::qualified("math", "add")),
                Some(Origin {
                    codec: sim_kernel::CodecId(3),
                    source: SourceId("tree.bin".to_owned()),
                    span: Span { start: 0, end: 1 },
                    trivia: Vec::new(),
                }),
            ),
            LocatedExprTree::without_children(
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "1".to_owned(),
                }),
                Some(Origin {
                    codec: sim_kernel::CodecId(3),
                    source: SourceId("tree.bin".to_owned()),
                    span: Span { start: 2, end: 3 },
                    trivia: Vec::new(),
                }),
            ),
            LocatedExprTree::without_children(
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                }),
                Some(Origin {
                    codec: sim_kernel::CodecId(3),
                    source: SourceId("tree.bin".to_owned()),
                    span: Span { start: 4, end: 5 },
                    trivia: Vec::new(),
                }),
            ),
        ],
    };

    let BinaryFrame(bytes) = encode_located_tree_frame(&tree, true).unwrap();
    let (_tables, decoded) = decode_located_tree_frame(sim_kernel::CodecId(3), &bytes).unwrap();
    assert_eq!(decoded, tree);
}

#[test]
fn tree_frame_encode_rejects_malformed_tree() {
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

    let err = encode_located_tree_frame(&tree, false).unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(message.contains("call tree expected 2 children"));
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn decode_enforces_limits() {
    let BinaryFrame(bytes) = encode_frame(&Expr::String("wire".repeat(8))).unwrap();
    let err = decode_located_tree_frame_with_limits(
        sim_kernel::CodecId(3),
        &bytes,
        DecodeLimits {
            max_string_bytes: 4,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(message.contains("string exceeds decode limit"));
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn decode_bounds_deeply_nested_count_bomb() {
    // Each level is a list that declares 65536 elements but supplies only its
    // first (a deeper list). Before the allocation cap, the eager
    // `Vec::with_capacity(65536)` at every level forced gigabytes of
    // simultaneous reservations; now each reservation is capped and the depth
    // budget fails the decode closed without ballooning memory or crashing.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SLB8"); // MAGIC
    bytes.push(0x01); // version
    bytes.push(0x00); // flags (no origin)
    bytes.push(0x00); // lib table length
    bytes.push(0x00); // symbol table length
    bytes.push(0x00); // number-domain table length
    for _ in 0..200 {
        bytes.push(0x07); // BinaryTag::List
        bytes.extend_from_slice(&[0x80, 0x80, 0x04]); // varuint(65_536)
    }
    let err = decode_located_tree_frame_with_limits(
        sim_kernel::CodecId(1),
        &bytes,
        DecodeLimits {
            max_depth: 64,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(
                message.contains("nesting depth"),
                "expected depth error, got: {message}"
            );
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn decode_header_table_count_does_not_allocate_above_clamp() {
    // A ~9-byte frame whose symbol-table header declares 65535 entries but
    // supplies none. The count is within the table-entry limit, so before the
    // ALLOC_RESERVE_CAP clamp the header eagerly reserved a 65535-slot `Vec`
    // (multiple MiB) for a truncated frame; now the reservation is capped and
    // the decode fails closed on the missing records without ballooning memory.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SLB8"); // MAGIC
    bytes.push(0x01); // version
    bytes.push(0x00); // flags (no origin)
    bytes.push(0x00); // lib table length
    bytes.extend_from_slice(&[0xff, 0xff, 0x03]); // symbol table length = varuint(65_535)
    // no symbol records follow: the frame is truncated here.
    let err = decode_located_tree_frame_with_limits(
        sim_kernel::CodecId(1),
        &bytes,
        DecodeLimits::default(),
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(
                message.contains("unexpected end of binary frame"),
                "expected truncation error, got: {message}"
            );
        }
        other => panic!("unexpected error {other:?}"),
    }
}
