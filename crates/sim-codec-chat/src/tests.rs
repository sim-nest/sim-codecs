use std::sync::Arc;

use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{DefaultFactory, EagerPolicy, EncodeOptions, Error, Expr, ReadPolicy, Symbol};

use crate::{
    ChatCodecLib, OllamaRequestOptions, decode_ollama_response, decode_ollama_stream,
    encode_ollama_request, is_model_request_expr, model_card_expr, model_error_expr,
    model_request_messages_expr, model_response_expr,
};

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let chat = ChatCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&chat).unwrap();
    let lisp = sim_codec_lisp::LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    let json = sim_codec_json::JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&json).unwrap();
    cx
}

fn request_expr() -> Expr {
    Expr::Map(vec![
        (Expr::Symbol(Symbol::new("model-request")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("task")),
            Expr::String("summarize this file".to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("messages")),
            Expr::List(vec![
                message_expr("system", "Answer in precise prose."),
                message_expr("user", "Summarize src/lib.rs"),
            ]),
        ),
    ])
}

fn response_expr() -> Expr {
    model_response_expr(
        Symbol::new("local-reasoner"),
        "qwen2.5-coder:14b",
        vec![content_part("The file defines the SIM root crate exports.")],
        Symbol::new("stop"),
    )
}

fn message_expr(role: &str, text: &str) -> Expr {
    Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("role")),
            Expr::Symbol(Symbol::new(role.to_owned())),
        ),
        (
            Expr::Symbol(Symbol::new("content")),
            Expr::List(vec![content_part(text)]),
        ),
    ])
}

fn content_part(text: &str) -> Expr {
    Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("type")),
            Expr::Symbol(Symbol::new("text")),
        ),
        (
            Expr::Symbol(Symbol::new("text")),
            Expr::String(text.to_owned()),
        ),
    ])
}

#[test]
fn request_and_response_roundtrip_through_chat_lisp_and_json() {
    let mut cx = cx();
    for expr in [request_expr(), response_expr()] {
        for codec in [
            Symbol::qualified("codec", "chat"),
            Symbol::qualified("codec", "lisp"),
            Symbol::qualified("codec", "json"),
        ] {
            let decoded = sim_test_support::roundtrip_sym(&mut cx, &codec, &expr);
            assert!(
                decoded.canonical_eq(&expr),
                "codec {codec} changed {expr:?} into {decoded:?}"
            );
        }
    }
}

#[test]
fn chat_encoding_is_stable_for_reordered_maps() {
    let mut cx = cx();
    let mut reversed = request_expr();
    if let Expr::Map(entries) = &mut reversed {
        entries.reverse();
    }
    let left = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "chat"),
        &request_expr(),
        EncodeOptions::default(),
    )
    .unwrap();
    let right = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "chat"),
        &reversed,
        EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(left, right);
}

#[test]
fn malformed_transcript_decode_returns_clear_eval_error() {
    let mut cx = cx();
    let malformed = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("model-request")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("task")),
            Expr::String("missing messages".to_owned()),
        ),
    ]);
    let text = crate::expr::encode_chat_text(&malformed);
    let err = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "chat"),
        Input::Text(text),
        ReadPolicy::default(),
    )
    .unwrap_err();
    match err {
        Error::Eval(message) => assert!(message.contains("messages"), "{message}"),
        other => panic!("expected Eval error, found {other:?}"),
    }
}

#[test]
fn malformed_wire_returns_codec_error() {
    let mut cx = cx();
    let err = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "chat"),
        Input::Text("not-chat".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap_err();
    match err {
        Error::CodecError { message, .. } => assert!(message.contains("SIMCHAT1"), "{message}"),
        other => panic!("expected codec error, found {other:?}"),
    }
}

#[test]
fn chat_encoder_rejects_non_chat_exprs() {
    let mut cx = cx();
    let err = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "chat"),
        &Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::new("not-chat"))),
            args: vec![Expr::String("payload".to_owned())],
        },
        EncodeOptions::default(),
    )
    .unwrap_err();

    match err {
        Error::Eval(message) => assert!(message.contains("chat transcript"), "{message}"),
        other => panic!("expected Eval error, found {other:?}"),
    }
}

#[test]
fn helper_functions_build_valid_transcripts() {
    let request = request_expr();
    assert!(is_model_request_expr(&request));
    assert_eq!(model_request_messages_expr(&request).unwrap().len(), 2);

    let response = response_expr();
    crate::validate_chat_transcript(&response).unwrap();

    let error = model_error_expr(Symbol::new("runner"), "fake/model", "no scripted response");
    crate::validate_chat_transcript(&error).unwrap();

    let card = model_card_expr(
        Symbol::new("runner"),
        "fake/model",
        Symbol::new("fake"),
        Symbol::new("local"),
    );
    crate::validate_chat_transcript(&card).unwrap();
}

#[test]
fn ollama_request_encoder_matches_fixture_shape() {
    let body = encode_ollama_request(
        &request_expr(),
        &OllamaRequestOptions::new("qwen3.5:4b", true, false),
    )
    .unwrap();
    let text = String::from_utf8(body).unwrap();
    assert!(text.contains("\"model\":\"qwen3.5:4b\""));
    assert!(text.contains("\"stream\":true"));
    assert!(text.contains("\"role\":\"system\""));
    assert!(text.contains("\"Summarize src/lib.rs\""));
}

#[test]
fn ollama_response_decoder_matches_chat_and_generate_shapes() {
    let chat = decode_ollama_response(
        Symbol::new("local"),
        "qwen3.5:4b",
        br#"{"model":"qwen3.5:4b","message":{"role":"assistant","content":"chat ok"},"done":true,"done_reason":"stop","prompt_eval_count":8,"eval_count":2}"#,
        true,
    )
    .unwrap();
    crate::validate_chat_transcript(&chat).unwrap();
    assert!(format!("{chat:?}").contains("chat ok"));
    assert!(format!("{chat:?}").contains("raw-provider-response"));

    let generate = decode_ollama_response(
        Symbol::new("local"),
        "qwen3.5:4b",
        br#"{"model":"qwen3.5:4b","response":"generate ok","done":true,"done_reason":"stop","prompt_eval_count":5,"eval_count":3}"#,
        false,
    )
    .unwrap();
    crate::validate_chat_transcript(&generate).unwrap();
    assert!(format!("{generate:?}").contains("generate ok"));
    assert!(format!("{generate:?}").contains("input-tokens"));
}

#[test]
fn ollama_stream_decoder_combines_buffered_chunks() {
    let expr = decode_ollama_stream(
        Symbol::new("local"),
        "qwen3.5:4b",
        br#"{"model":"qwen3.5:4b","message":{"role":"assistant","content":"hello "},"done":false}
{"model":"qwen3.5:4b","message":{"role":"assistant","content":"world"},"done":false}
{"model":"qwen3.5:4b","done":true,"done_reason":"stop","prompt_eval_count":6,"eval_count":2}"#,
        true,
    )
    .unwrap();
    crate::validate_chat_transcript(&expr).unwrap();
    assert!(format!("{expr:?}").contains("hello world"));
    assert!(format!("{expr:?}").contains("raw-provider-response"));
    assert!(format!("{expr:?}").contains("output-tokens"));
}

#[test]
fn ollama_response_decoder_bounds_oversized_raw_projection() {
    // The raw-provider-response projection now runs under a decode budget; a
    // provider array larger than the collection-length budget must fail closed
    // rather than projecting an unbounded Expr.
    let mut body = String::from(r#"{"response":"ok","done":true,"huge":["#);
    for _ in 0..70_000 {
        body.push_str("0,");
    }
    body.push_str("0]}");
    let err = decode_ollama_response(Symbol::new("local"), "m", body.as_bytes(), true).unwrap_err();
    assert!(
        matches!(err, Error::CodecError { ref message, .. } if message.contains("collection length")),
        "expected collection-length budget error, got {err:?}"
    );
}

#[test]
fn chat_base64_rejects_noncanonical_padding() {
    use crate::base64::base64_decode;
    let codec = sim_kernel::CodecId(1);
    // Interior padding, pad-then-non-pad, and leading pad must all fail closed.
    for bad in ["AA=A", "AA==AAAA", "=AAA", "A=AA", "AAAA=AAA"] {
        assert!(
            base64_decode(codec, bad).is_err(),
            "accepted bad pad: {bad}"
        );
    }
    // Canonical encodings still decode.
    assert_eq!(base64_decode(codec, "Zm9v").unwrap(), b"foo");
    assert_eq!(base64_decode(codec, "Zg==").unwrap(), b"f");
}

#[test]
fn ollama_usage_token_total_saturates_without_overflow() {
    // prompt_eval_count + eval_count is a u64 add over attacker-controlled
    // numbers; it must saturate instead of overflowing and panicking.
    let body = format!(
        r#"{{"response":"ok","done":true,"prompt_eval_count":{max},"eval_count":{max}}}"#,
        max = u64::MAX
    );
    let expr = decode_ollama_response(Symbol::new("local"), "m", body.as_bytes(), false).unwrap();
    // u64::MAX saturated total round-trips through the usage record.
    assert!(format!("{expr:?}").contains(&u64::MAX.to_string()));
}
