use std::sync::Arc;

use sim_codec::{
    DecodeLimits, DecodePosition, DecodedForm, Input, Output, decode_datum_with_codec,
    decode_default_with_codec, decode_tree_with_codec, decode_with_codec,
    decode_with_codec_and_limits, encode_datum_with_codec, encode_located_with_codec,
    encode_tree_with_codec, encode_with_codec,
};
use sim_codec_binary_base64::{decode_base64, encode_base64};
use sim_kernel::{
    Datum, DefaultFactory, EagerPolicy, EncodeOptions, Expr, LocatedExpr, LocatedExprTree,
    NumberLiteral, Origin, QuoteMode, ReadPolicy, SourceId, Span, Symbol, Trivia,
};

use crate::BitwiseBase64CodecLib;

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let bitwise = sim_codec_bitwise::BitwiseCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&bitwise).unwrap();
    let bitwise_base64 = BitwiseBase64CodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&bitwise_base64).unwrap();
    cx
}

fn symbol() -> Symbol {
    Symbol::qualified("codec", "bitwise-base64")
}

fn corpus() -> Vec<Expr> {
    vec![
        Expr::Nil,
        Expr::Bool(true),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "42.5".to_owned(),
        }),
        Expr::Symbol(Symbol::qualified("math", "pi")),
        Expr::String("line\n\"quoted\"".to_owned()),
        Expr::Bytes(vec![0, 1, 2, 0xff]),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("f")),
            Expr::String("x".to_owned()),
            Expr::Bool(false),
        ]),
        Expr::Map(vec![
            (Expr::Symbol(Symbol::new("b")), Expr::Bool(false)),
            (Expr::Symbol(Symbol::new("a")), Expr::Bool(true)),
        ]),
        Expr::Set(vec![
            Expr::String("z".to_owned()),
            Expr::String("a".to_owned()),
        ]),
        Expr::Quote {
            mode: QuoteMode::Syntax,
            expr: Box::new(Expr::Extension {
                tag: Symbol::qualified("demo", "escape"),
                payload: Box::new(Expr::Annotated {
                    expr: Box::new(Expr::Vector(vec![Expr::Bool(true)])),
                    annotations: vec![(
                        Symbol::qualified("meta", "origin"),
                        Expr::String("matrix".to_owned()),
                    )],
                }),
            }),
        },
    ]
}

#[test]
fn codec_registers() {
    let cx = cx();
    assert!(cx.registry().codec_by_symbol(&symbol()).is_some());
}

#[test]
fn encoder_returns_ascii_text_with_no_line_breaks() {
    let mut cx = cx();
    let output = encode_with_codec(
        &mut cx,
        &symbol(),
        &Expr::Bytes((0..=255).collect()),
        EncodeOptions::default(),
    )
    .unwrap();
    let Output::Text(text) = output else {
        panic!("bitwise-base64 must produce text output");
    };
    assert!(text.is_ascii());
    assert!(!text.contains('\n'));
    assert!(!text.contains('\r'));
}

#[test]
fn full_expr_surface_roundtrips_as_ascii() {
    let mut cx = cx();
    for expr in corpus() {
        let text = encode_with_codec(&mut cx, &symbol(), &expr, EncodeOptions::default())
            .unwrap()
            .into_text()
            .unwrap();
        assert!(text.is_ascii(), "wrapper output must be ASCII for {expr:?}");
        let decoded = sim_test_support::roundtrip(&mut cx, "bitwise-base64", &expr);
        assert!(
            decoded.canonical_eq(&expr),
            "decoded {decoded:?} from {expr:?}"
        );
    }
}

#[test]
fn emitted_text_is_base64_of_bitwise_frame() {
    let mut cx = cx();
    let expr = Expr::String("wire".to_owned());
    let base64_output = encode_with_codec(&mut cx, &symbol(), &expr, EncodeOptions::default())
        .unwrap()
        .into_text()
        .unwrap();
    let frame = sim_codec_bitwise::encode_frame(&expr).unwrap();
    assert_eq!(
        decode_base64(sim_kernel::CodecId(9), &base64_output).unwrap(),
        frame.0
    );
}

#[test]
fn datum_roundtrip_preserves_content_id() {
    let mut cx = cx();
    let datum = sample_datum();
    let content_id = datum.content_id().unwrap();

    let output = encode_datum_with_codec(&mut cx, &symbol(), &datum, EncodeOptions::default())
        .unwrap()
        .into_text()
        .unwrap();
    let decoded = decode_datum_with_codec(
        &mut cx,
        &symbol(),
        Input::Text(output),
        ReadPolicy::default(),
    )
    .unwrap();

    assert_eq!(decoded, datum);
    assert_eq!(decoded.content_id().unwrap(), content_id);
}

#[test]
fn default_decode_returns_datum_even_in_eval_position() {
    let mut cx = cx();
    let datum = sample_datum();
    let output = encode_datum_with_codec(&mut cx, &symbol(), &datum, EncodeOptions::default())
        .unwrap()
        .into_text()
        .unwrap();

    let decoded = decode_default_with_codec(
        &mut cx,
        &symbol(),
        Input::Text(output),
        ReadPolicy::default(),
        DecodePosition::Eval,
    )
    .unwrap();

    assert_eq!(decoded, DecodedForm::Datum(datum));
}

#[test]
fn decode_accepts_ascii_whitespace_around_base64() {
    let mut cx = cx();
    let text = encode_with_codec(
        &mut cx,
        &symbol(),
        &Expr::String("space".to_owned()),
        EncodeOptions::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    let spaced = format!(" \n{}\t\r", text);
    let decoded = decode_with_codec(
        &mut cx,
        &symbol(),
        Input::Text(spaced),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(decoded, Expr::String("space".to_owned()));
}

#[test]
fn malformed_base64_fails_closed() {
    let mut cx = cx();
    for text in ["@@==", "abc", "ab=c", "abcd====", "Zh=="] {
        let err = decode_with_codec(
            &mut cx,
            &symbol(),
            Input::Text(text.to_owned()),
            ReadPolicy::default(),
        )
        .unwrap_err();
        match err {
            sim_kernel::Error::CodecError { message, .. } => {
                assert!(message.contains("base64"), "{message}");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }
}

#[test]
fn valid_base64_of_junk_bytes_fails_closed() {
    let mut cx = cx();
    let err = decode_with_codec(
        &mut cx,
        &symbol(),
        Input::Text(encode_base64(b"\xff\xff\xff\xff")),
        ReadPolicy::default(),
    )
    .unwrap_err();
    assert!(matches!(err, sim_kernel::Error::CodecError { .. }));
}

#[test]
fn bytes_input_must_be_utf8() {
    let mut cx = cx();
    let err = decode_with_codec(
        &mut cx,
        &symbol(),
        Input::Bytes(vec![0xff]),
        ReadPolicy::default(),
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(message.contains("not valid UTF-8"));
        }
        other => panic!("unexpected error {other:?}"),
    }
}

fn sample_datum() -> Datum {
    Datum::Map(vec![
        (
            Datum::Symbol(Symbol::new("codec")),
            Datum::Symbol(Symbol::qualified("codec", "bitwise-base64")),
        ),
        (
            Datum::Symbol(Symbol::new("payload")),
            Datum::List(vec![Datum::Bool(false), Datum::Bytes(vec![0, 1, 2, 3])]),
        ),
    ])
}

#[test]
fn located_expr_roundtrips_with_origin() {
    let mut cx = cx();
    let located = LocatedExpr {
        expr: Expr::String("wire".to_owned()),
        origin: Some(Origin {
            codec: sim_kernel::CodecId(3),
            source: SourceId("cache.bitb64".to_owned()),
            span: Span { start: 10, end: 14 },
            trivia: vec![
                Trivia::Whitespace(" ".to_owned()),
                Trivia::BlockComment("/*x*/".to_owned()),
            ],
        }),
    };

    let encoded = encode_located_with_codec(
        &mut cx,
        &symbol(),
        &located,
        EncodeOptions {
            lossless_origin: true,
            ..Default::default()
        },
    )
    .unwrap();
    let decoded = sim_codec::decode_located_with_codec(
        &mut cx,
        &symbol(),
        match encoded {
            Output::Text(text) => Input::Text(text),
            Output::Bytes(bytes) => Input::Bytes(bytes),
        },
        ReadPolicy::default(),
        "ignored.bitb64",
    )
    .unwrap();
    assert_eq!(decoded, located);
}

#[test]
fn tree_roundtrips_nested_origins() {
    let mut cx = cx();
    let tree = LocatedExprTree {
        expr: Expr::List(vec![Expr::String("x".to_owned())]),
        origin: Some(Origin {
            codec: sim_kernel::CodecId(7),
            source: SourceId("root".to_owned()),
            span: Span { start: 0, end: 3 },
            trivia: vec![Trivia::Whitespace(" ".to_owned())],
        }),
        children: vec![LocatedExprTree::without_children(
            Expr::String("x".to_owned()),
            Some(Origin {
                codec: sim_kernel::CodecId(7),
                source: SourceId("child".to_owned()),
                span: Span { start: 1, end: 2 },
                trivia: vec![Trivia::LineComment("; child".to_owned())],
            }),
        )],
    };

    let encoded = encode_tree_with_codec(
        &mut cx,
        &symbol(),
        &tree,
        EncodeOptions {
            lossless_origin: true,
            ..Default::default()
        },
    )
    .unwrap();
    let decoded = decode_tree_with_codec(
        &mut cx,
        &symbol(),
        match encoded {
            Output::Text(text) => Input::Text(text),
            Output::Bytes(bytes) => Input::Bytes(bytes),
        },
        ReadPolicy::default(),
        "ignored.bitb64",
    )
    .unwrap();
    assert_eq!(decoded, tree);
}

#[test]
fn decode_enforces_underlying_bitwise_limits() {
    let mut cx = cx();
    let text = encode_with_codec(
        &mut cx,
        &symbol(),
        &Expr::String("wire".repeat(8)),
        EncodeOptions::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    let err = decode_with_codec_and_limits(
        &mut cx,
        &symbol(),
        Input::Text(text),
        ReadPolicy::default(),
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
fn decode_rejects_oversized_whitespace_heavy_input_before_allocation() {
    let mut cx = cx();
    // A tiny amount of real base64 buried in a large whitespace pad. The raw
    // input length (>> max_input_bytes) must be rejected before the wrapper
    // allocates the whitespace-stripping buffer or the decoded frame buffer.
    let padded = format!("{}QQ=={}", " ".repeat(4096), "\n".repeat(4096));
    let err = decode_with_codec_and_limits(
        &mut cx,
        &symbol(),
        Input::Text(padded),
        ReadPolicy::default(),
        DecodeLimits {
            max_input_bytes: 64,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();
    match err {
        sim_kernel::Error::CodecError { message, .. } => {
            assert!(
                message.contains("input bytes limit exceeded"),
                "unexpected message {message:?}"
            );
        }
        other => panic!("unexpected error {other:?}"),
    }
}
