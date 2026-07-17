use std::sync::Arc;

use serde_json::{Value as JsonValue, json};
use sim_codec::{
    CodecRuntime, DecodeBudget, DecodeLimits, DecodePosition, DecodedForm, Input,
    decode_datum_with_codec, decode_default_with_codec, decode_with_codec,
    decode_with_codec_and_limits, encode_datum_with_codec, encode_tree_with_codec,
};
use sim_kernel::{
    Datum, DefaultFactory, EagerPolicy, EncodeOptions, Expr, LocatedExpr, LocatedExprTree,
    NumberLiteral, Origin, QuoteMode, ReadPolicy, SourceId, Span, Symbol, Trivia,
};

use crate::helpers::base64_encode;
use crate::{
    JsonCodecLib, expr_to_json, json_escape, json_to_expr, json_to_located_expr,
    located_expr_to_json,
};

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let lib = JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

#[test]
fn codec_registers() {
    let cx = cx();
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "json"))
            .is_some()
    );
}

fn codec_id(cx: &mut sim_kernel::Cx, symbol: &Symbol) -> sim_kernel::CodecId {
    cx.resolve_codec(symbol)
        .unwrap()
        .object()
        .as_any()
        .downcast_ref::<CodecRuntime>()
        .unwrap()
        .id
}

fn assert_codec_error(err: sim_kernel::Error, expected: sim_kernel::CodecId, needle: &str) {
    match err {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, expected);
            assert_ne!(codec, sim_kernel::CodecId(0));
            assert!(message.contains(needle), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn invalid_utf8_input_reports_json_codec_id() {
    let mut cx = cx();
    let symbol = Symbol::qualified("codec", "json");
    let expected = codec_id(&mut cx, &symbol);

    let err = decode_with_codec(
        &mut cx,
        &symbol,
        Input::Bytes(vec![0xff]),
        ReadPolicy::default(),
    )
    .unwrap_err();

    assert_codec_error(err, expected, "not valid UTF-8");
}

#[test]
fn json_escape_returns_a_json_string_fragment() {
    assert_eq!(
        json_escape("quote\" slash\\ lf\n cr\r tab\t back\u{08} form\u{0c} nul\u{0}"),
        "quote\\\" slash\\\\ lf\\n cr\\r tab\\t back\\b form\\f nul\\u0000"
    );
    assert_eq!(json_escape("plain utf8 cafe"), "plain utf8 cafe");
}

#[test]
fn call_roundtrips() {
    let mut cx = cx();
    let expr = Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::new("+"))),
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
    };

    assert_eq!(sim_test_support::roundtrip(&mut cx, "json", &expr), expr);
}

#[test]
fn full_expr_surface_roundtrips() {
    let mut cx = cx();
    let expr = Expr::Annotated {
        expr: Box::new(Expr::Extension {
            tag: Symbol::qualified("demo", "tag"),
            payload: Box::new(Expr::Map(vec![
                (
                    Expr::Symbol(Symbol::new("x")),
                    Expr::Vector(vec![
                        Expr::Bool(true),
                        Expr::Bytes(vec![1, 2, 3, 4]),
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
                ),
                (
                    Expr::String("set".to_owned()),
                    Expr::Set(vec![
                        Expr::String("b".to_owned()),
                        Expr::String("a".to_owned()),
                    ]),
                ),
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

    let decoded = sim_test_support::roundtrip(&mut cx, "json", &expr);
    assert!(decoded.canonical_eq(&expr));
}

#[test]
fn datum_roundtrip_preserves_content_id() {
    let mut cx = cx();
    let datum = sample_datum();
    let content_id = datum.content_id().unwrap();

    let output = encode_datum_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "json"),
        &datum,
        EncodeOptions::default(),
    )
    .unwrap();
    let decoded = decode_datum_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "json"),
        Input::Text(output.into_text().unwrap()),
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
        &Symbol::qualified("codec", "json"),
        &datum,
        EncodeOptions::default(),
    )
    .unwrap();

    let decoded = decode_default_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "json"),
        Input::Text(output.into_text().unwrap()),
        Default::default(),
        DecodePosition::Eval,
    )
    .unwrap();

    assert_eq!(decoded, DecodedForm::Datum(datum));
}

#[test]
fn slash_named_symbols_round_trip_structurally() {
    // The division operator is the symbol `/`; flattening operators through
    // `to_string()`/`split_once('/')` mangled it to `qualified("", "")`. The
    // structured symbol form must round-trip it (and every other symbol-bearing
    // position) exactly.
    let mut cx = cx();
    let expr = Expr::Annotated {
        expr: Box::new(Expr::Extension {
            tag: Symbol::new("/"),
            payload: Box::new(Expr::Infix {
                operator: Symbol::new("/"),
                left: Box::new(Expr::Prefix {
                    operator: Symbol::new("/"),
                    arg: Box::new(Expr::Local(Symbol::new("/"))),
                }),
                right: Box::new(Expr::Postfix {
                    operator: Symbol::qualified("ops", "/"),
                    arg: Box::new(Expr::Symbol(Symbol::new("/"))),
                }),
            }),
        }),
        annotations: vec![(Symbol::new("/"), Expr::String("v".to_owned()))],
    };
    let decoded = sim_test_support::roundtrip(&mut cx, "json", &expr);
    assert!(decoded.canonical_eq(&expr), "got {decoded:?}");

    // Direct encode/decode also round-trips the bare division operator.
    let infix = Expr::Infix {
        operator: Symbol::new("/"),
        left: Box::new(Expr::Symbol(Symbol::new("a"))),
        right: Box::new(Expr::Symbol(Symbol::new("b"))),
    };
    let json = expr_to_json(&infix);
    let back = json_to_expr(
        sim_kernel::CodecId(1),
        &json,
        &mut DecodeBudget::new(DecodeLimits::default()),
        0,
    )
    .unwrap();
    assert_eq!(back, infix);
}

#[test]
fn legacy_string_operator_still_decodes() {
    // Old payloads encoded operators/tags as bare strings; decode must remain
    // backward compatible with that flattened form.
    let legacy = json!({
        "$expr": "infix",
        "operator": "+",
        "left": { "$expr": "symbol", "name": "a" },
        "right": { "$expr": "symbol", "name": "b" },
    });
    let decoded = json_to_expr(
        sim_kernel::CodecId(1),
        &legacy,
        &mut DecodeBudget::new(DecodeLimits::default()),
        0,
    )
    .unwrap();
    assert_eq!(
        decoded,
        Expr::Infix {
            operator: Symbol::new("+"),
            left: Box::new(Expr::Symbol(Symbol::new("a"))),
            right: Box::new(Expr::Symbol(Symbol::new("b"))),
        }
    );
}

#[test]
fn bytes_use_base64() {
    let json = expr_to_json(&Expr::Bytes(vec![0xfb, 0xef]));
    assert_eq!(json["base64"], JsonValue::String("++8=".to_owned()));
    assert_eq!(
        json_to_expr(
            sim_kernel::CodecId(1),
            &json,
            &mut DecodeBudget::new(DecodeLimits::default()),
            0,
        )
        .unwrap(),
        Expr::Bytes(vec![0xfb, 0xef])
    );
}

#[test]
fn base64_rejects_noncanonical_padding() {
    use crate::helpers::base64_decode;
    let codec = sim_kernel::CodecId(1);
    for bad in ["AA=A", "AA==AAAA", "=AAA", "A=AA", "AAAA=AAA"] {
        assert!(
            base64_decode(codec, bad).is_err(),
            "accepted bad padding: {bad}"
        );
    }
    // Canonical encodings still decode.
    assert_eq!(base64_decode(codec, "Zm9v").unwrap(), b"foo");
    assert_eq!(base64_decode(codec, "Zg==").unwrap(), b"f");
}

fn sample_datum() -> Datum {
    Datum::Map(vec![
        (
            Datum::Symbol(Symbol::new("name")),
            Datum::String("json".to_owned()),
        ),
        (
            Datum::Symbol(Symbol::new("payload")),
            Datum::Vector(vec![Datum::Bool(true), Datum::Bytes(vec![0xfb, 0xef])]),
        ),
    ])
}

#[test]
fn unknown_tags_fail() {
    let err = json_to_expr(
        sim_kernel::CodecId(7),
        &json!({ "$expr": "mystery" }),
        &mut DecodeBudget::new(DecodeLimits::default()),
        0,
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, sim_kernel::CodecId(7));
            assert!(message.contains("unknown expr tag mystery"));
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn located_expr_roundtrips_with_origin() {
    let located = LocatedExpr {
        expr: Expr::String("hello".to_owned()),
        origin: Some(Origin {
            codec: sim_kernel::CodecId(9),
            source: SourceId("test.lisp".to_owned()),
            span: Span { start: 3, end: 8 },
            trivia: vec![
                Trivia::Whitespace(" ".to_owned()),
                Trivia::LineComment("; hi".to_owned()),
            ],
        }),
    };

    let value = located_expr_to_json(&located, true);
    let decoded = json_to_located_expr(
        sim_kernel::CodecId(9),
        &value,
        &mut DecodeBudget::new(DecodeLimits::default()),
        0,
    )
    .unwrap();
    assert_eq!(decoded, located);
}

#[test]
fn json_decode_rejects_excessive_collection_len() {
    let mut cx = cx();
    let limits = DecodeLimits {
        max_collection_len: 1,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "json"),
        Input::Text(
            "{\"$expr\":\"list\",\"items\":[{\"$expr\":\"nil\"},{\"$expr\":\"nil\"}]}".to_owned(),
        ),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("collection length"))
    );
}

#[test]
fn json_decode_rejects_excessive_depth() {
    let mut cx = cx();
    let mut wrapped = "{\"$expr\":\"nil\"}".to_owned();
    for _ in 0..10 {
        wrapped = format!("{{\"$expr\":\"list\",\"items\":[{wrapped}]}}");
    }
    let limits = DecodeLimits {
        max_depth: 4,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "json"),
        Input::Text(wrapped),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("recursion depth"))
    );
}

#[test]
fn json_decode_rejects_excessive_blob_bytes() {
    let mut cx = cx();
    let payload = base64_encode(&[0u8; 32]);
    let limits = DecodeLimits {
        max_blob_bytes: 8,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "json"),
        Input::Text(format!("{{\"$expr\":\"bytes\",\"base64\":\"{payload}\"}}")),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("blob bytes"))
    );
}

#[test]
fn malformed_tree_encode_returns_error() {
    let mut cx = cx();
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

    let err = encode_tree_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "json"),
        &tree,
        EncodeOptions {
            lossless_origin: true,
            ..EncodeOptions::default()
        },
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(message.contains("call tree expected 2 children"));
        }
        other => panic!("unexpected error {other:?}"),
    }
}
