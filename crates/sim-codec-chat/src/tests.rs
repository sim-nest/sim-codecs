use std::sync::Arc;

use sim_codec::{
    DecodeLimits, Input, decode_with_codec, decode_with_codec_and_limits, encode_with_codec,
};
use sim_kernel::{
    Args, DefaultFactory, EagerPolicy, EncodeOptions, Error, Expr, ReadPolicy, Symbol,
};

use crate::{
    AnthropicCodecLib, ChatCodecLib, LemonadeCodecLib, LmStudioCodecLib, OllamaCodecLib,
    OpenAiCodecLib, OpenAiRequestOptions, RequestWire, StreamWire, decode_openai_response,
    decode_openai_response_with_limits, decode_openai_stream, decode_openai_stream_with_limits,
    encode_openai_request, is_model_request_expr, model_card_expr, model_error_expr,
    model_request_messages_expr, model_response_expr,
};

mod anthropic;
mod ollama;
mod openai_compat;

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let chat = ChatCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&chat).unwrap();
    let openai = OpenAiCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&openai).unwrap();
    let anthropic = AnthropicCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&anthropic).unwrap();
    let lm_studio = LmStudioCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lm_studio).unwrap();
    let lemonade = LemonadeCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lemonade).unwrap();
    let ollama = OllamaCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&ollama).unwrap();
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
fn provider_profiles_are_open_data_records() {
    let openai = crate::openai_profile();
    assert_eq!(openai.codec, Symbol::qualified("codec", "openai"));
    assert_eq!(openai.provider, Symbol::new("openai"));
    assert_eq!(openai.request_wire, RequestWire::OpenAiChat);
    assert_eq!(openai.stream_wire, StreamWire::Sse);

    let ollama = crate::ollama_profile();
    assert_eq!(ollama.codec, Symbol::qualified("codec", "ollama"));
    assert_eq!(ollama.provider, Symbol::new("ollama"));
    assert_eq!(ollama.request_wire, RequestWire::OllamaChat);
    assert_eq!(ollama.stream_wire, StreamWire::Ndjson);

    let anthropic = crate::anthropic_profile();
    assert_eq!(anthropic.codec, Symbol::qualified("codec", "anthropic"));
    assert_eq!(anthropic.provider, Symbol::new("anthropic"));
    assert_eq!(anthropic.request_wire, RequestWire::AnthropicMessages);
    assert_eq!(anthropic.stream_wire, StreamWire::Sse);

    let lm_studio = crate::lm_studio_profile();
    assert_eq!(lm_studio.codec, Symbol::qualified("codec", "lm-studio"));
    assert_eq!(lm_studio.provider, Symbol::new("lm-studio"));
    assert_eq!(lm_studio.request_wire, RequestWire::OpenAiChat);
    assert_eq!(lm_studio.stream_wire, StreamWire::Sse);

    let lemonade = crate::lemonade_profile();
    assert_eq!(lemonade.codec, Symbol::qualified("codec", "lemonade"));
    assert_eq!(lemonade.provider, Symbol::new("lemonade"));
    assert_eq!(lemonade.request_wire, RequestWire::OpenAiChat);
    assert_eq!(lemonade.stream_wire, StreamWire::Sse);
}

#[test]
fn cookbook_profile_and_transcript_functions_run() {
    let mut cx = cx();
    let transcript = call_report(&mut cx, Symbol::qualified("chat", "transcript-roundtrip"));
    assert_eq!(field_bool(&transcript, "roundtrip"), Some(true));
    assert_eq!(field_string(&transcript, "codec"), Some("codec/chat"));

    let profiles = call_report(&mut cx, Symbol::qualified("chat", "provider-profiles"));
    assert_eq!(field_string(&profiles, "count"), Some("5"));
}

#[test]
fn provider_runtime_codecs_install() {
    let mut cx = cx();
    assert!(
        cx.resolve_codec(&Symbol::qualified("codec", "openai"))
            .is_ok()
    );
    assert!(
        cx.resolve_codec(&Symbol::qualified("codec", "anthropic"))
            .is_ok()
    );
    assert!(
        cx.resolve_codec(&Symbol::qualified("codec", "lm-studio"))
            .is_ok()
    );
    assert!(
        cx.resolve_codec(&Symbol::qualified("codec", "lemonade"))
            .is_ok()
    );
    assert!(
        cx.resolve_codec(&Symbol::qualified("codec", "ollama"))
            .is_ok()
    );

    let response = response_expr();
    let openai_output = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "openai"),
        &response,
        EncodeOptions::default(),
    )
    .unwrap();
    assert!(
        openai_output
            .into_text()
            .unwrap()
            .contains("chat.completion")
    );

    let anthropic_output = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "anthropic"),
        &response,
        EncodeOptions::default(),
    )
    .unwrap();
    assert!(
        anthropic_output
            .into_text()
            .unwrap()
            .contains("\"type\":\"message\"")
    );

    let lm_studio_output = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lm-studio"),
        &response,
        EncodeOptions::default(),
    )
    .unwrap();
    assert!(
        lm_studio_output
            .into_text()
            .unwrap()
            .contains("chat.completion")
    );

    let lemonade_output = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lemonade"),
        &response,
        EncodeOptions::default(),
    )
    .unwrap();
    assert!(
        lemonade_output
            .into_text()
            .unwrap()
            .contains("chat.completion")
    );

    let ollama_output = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "ollama"),
        &request_expr(),
        EncodeOptions::default(),
    )
    .unwrap();
    assert!(
        ollama_output
            .into_text()
            .unwrap()
            .contains("\"model\":\"ollama\"")
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
fn openai_runtime_codec_decodes_chat_request() {
    let mut cx = cx();
    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "openai"),
        Input::Text(
            r#"{"model":"gpt-5-mini","messages":[{"role":"user","content":"hello"}]}"#.to_owned(),
        ),
        ReadPolicy::default(),
    )
    .unwrap();

    crate::validate_chat_transcript(&decoded).unwrap();
    assert!(format!("{decoded:?}").contains("model-request"));
    assert!(format!("{decoded:?}").contains("hello"));
}

#[test]
fn openai_runtime_request_honors_decode_input_limit() {
    let mut cx = cx();
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "openai"),
        Input::Text(
            r#"{"model":"gpt-5-mini","messages":[{"role":"user","content":"hello"}]}"#.to_owned(),
        ),
        ReadPolicy::default(),
        DecodeLimits {
            max_input_bytes: 8,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();

    assert!(
        matches!(err, Error::CodecError { ref message, .. } if message.contains("input bytes")),
        "expected input-byte budget error, got {err:?}"
    );
}

#[test]
fn openai_request_encoder_matches_fixture_shape() {
    let body = encode_openai_request(
        &request_expr(),
        &OpenAiRequestOptions::new("gpt-5-mini", true, true),
    )
    .unwrap();
    let text = String::from_utf8(body).unwrap();
    assert!(text.contains("\"model\":\"gpt-5-mini\""));
    assert!(text.contains("\"stream\":true"));
    assert!(text.contains("\"stream_options\":{\"include_usage\":true}"));
    assert!(text.contains("\"role\":\"system\""));
    assert!(text.contains("\"summarize this file\""));
}

#[test]
fn openai_response_decoder_matches_fixture_shape() {
    let expr = decode_openai_response(
        Symbol::new("remote"),
        "gpt-5-mini",
        br#"{"id":"chatcmpl-1","choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"compiled"}}],"usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}}"#,
        true,
    )
    .unwrap();

    crate::validate_chat_transcript(&expr).unwrap();
    assert!(format!("{expr:?}").contains("compiled"));
    assert!(format!("{expr:?}").contains("raw-provider-response"));
    assert!(format!("{expr:?}").contains("input-tokens"));
}

#[test]
fn openai_response_raw_projection_honors_decode_collection_limit() {
    let err = decode_openai_response_with_limits(
        Symbol::new("remote"),
        "gpt-5-mini",
        br#"{"id":"chatcmpl-1","choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"compiled"}}]}"#,
        true,
        DecodeLimits {
            max_collection_len: 0,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();

    assert!(
        matches!(err, Error::CodecError { ref message, .. } if message.contains("collection length")),
        "expected collection-length budget error, got {err:?}"
    );
}

#[test]
fn openai_response_decoder_decodes_tool_calls() {
    let expr = decode_openai_response(
        Symbol::new("remote"),
        "gpt-5-mini",
        br#"{"id":"chatcmpl-tool","choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"location\":\"Stockholm\",\"unit\":\"celsius\"}"}}]}}],"usage":{"prompt_tokens":21,"completion_tokens":6,"total_tokens":27}}"#,
        false,
    )
    .unwrap();

    crate::validate_chat_transcript(&expr).unwrap();
    let rendered = format!("{expr:?}");
    assert!(rendered.contains("tool-call"));
    assert!(rendered.contains("call_1"));
    assert!(rendered.contains("get_weather"));
    assert!(rendered.contains("Stockholm"));
    assert!(rendered.contains("tool_calls"));
}

#[test]
fn openai_stream_decoder_combines_sse_chunks() {
    let expr = decode_openai_stream(
        Symbol::new("remote"),
        "gpt-5-mini",
        br#"data: {"id":"chunk-1","choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chunk-1","choices":[{"delta":{"content":"hello "},"finish_reason":null}]}

data: {"id":"chunk-1","choices":[{"delta":{"content":"world"},"finish_reason":"stop"}]}

data: {"id":"chunk-1","choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}

data: [DONE]
"#,
        true,
    )
    .unwrap();

    crate::validate_chat_transcript(&expr).unwrap();
    assert!(format!("{expr:?}").contains("hello world"));
    assert!(format!("{expr:?}").contains("raw-provider-response"));
    assert!(format!("{expr:?}").contains("output-tokens"));
}

#[test]
fn openai_stream_honors_decode_input_limit() {
    let err = decode_openai_stream_with_limits(
        Symbol::new("remote"),
        "gpt-5-mini",
        br#"data: {"id":"chunk-1","choices":[{"delta":{"content":"hello"},"finish_reason":"stop"}]}

data: [DONE]
"#,
        false,
        DecodeLimits {
            max_input_bytes: 8,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();

    assert!(
        matches!(err, Error::CodecError { ref message, .. } if message.contains("input bytes")),
        "expected input-byte budget error, got {err:?}"
    );
}

#[test]
fn openai_stream_chunk_accumulation_honors_decode_collection_limit() {
    let err = decode_openai_stream_with_limits(
        Symbol::new("remote"),
        "gpt-5-mini",
        br#"data: {"id":"chunk-1","choices":[{"delta":{"content":"hello"},"finish_reason":"stop"}]}

data: [DONE]
"#,
        false,
        DecodeLimits {
            max_collection_len: 0,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();

    assert!(
        matches!(err, Error::CodecError { ref message, .. } if message.contains("collection length")),
        "expected collection-length budget error, got {err:?}"
    );
}

#[test]
fn openai_error_envelope_decodes_to_model_error() {
    let expr = decode_openai_response(
        Symbol::new("remote"),
        "gpt-5-mini",
        br#"{"error":{"message":"bad key","type":"invalid_request_error"}}"#,
        false,
    )
    .unwrap();

    crate::validate_chat_transcript(&expr).unwrap();
    assert!(format!("{expr:?}").contains("bad key"));
    assert!(format!("{expr:?}").contains("shape-ok"));
}

#[test]
fn openai_response_decoder_bounds_oversized_raw_projection() {
    let mut body = String::from(
        r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"huge":["#,
    );
    for _ in 0..70_000 {
        body.push_str("0,");
    }
    body.push_str("0]}");

    let err = decode_openai_response(Symbol::new("remote"), "gpt-5-mini", body.as_bytes(), true)
        .unwrap_err();
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
