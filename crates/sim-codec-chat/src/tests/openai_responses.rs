use sim_codec::{DecodeLimits, Input};
use sim_kernel::{Error, Expr, Symbol};
use sim_value::access::field as map_field;

use crate::{
    OpenAiRequestOptions, RequestWire, StreamWire, decode_openai_response,
    decode_openai_responses_request, decode_openai_responses_response,
    decode_openai_responses_response_with_limits, decode_openai_responses_stream,
    encode_openai_request, encode_openai_responses_request, openai_profile,
    openai_responses_profile, validate_chat_transcript,
};

use super::{request_expr, request_expr_with_extra};

const REQUEST: &[u8] = include_bytes!("fixtures/openai_responses/request.json");
const RESPONSE: &[u8] = include_bytes!("fixtures/openai_responses/response.json");
const STREAM: &[u8] = include_bytes!("fixtures/openai_responses/stream.sse");
const REFUSAL: &[u8] = include_bytes!("fixtures/openai_responses/refusal.json");
const STRUCTURED: &[u8] = include_bytes!("fixtures/openai_responses/structured.json");
const MALFORMED: &[u8] = include_bytes!("fixtures/openai_responses/malformed.sse");
const TRUNCATED: &[u8] = include_bytes!("fixtures/openai_responses/truncated.sse");

#[test]
fn responses_profile_is_explicit_and_shares_openai_identity() {
    let chat = openai_profile();
    let responses = openai_responses_profile();
    assert_eq!(chat.codec, responses.codec);
    assert_eq!(chat.provider, responses.provider);
    assert_eq!(chat.request_wire, RequestWire::OpenAiChat);
    assert_eq!(responses.request_wire, RequestWire::OpenAiResponses);
    assert_eq!(responses.stream_wire, StreamWire::Sse);
}

#[test]
fn responses_request_matches_golden_and_roundtrips() {
    let body = encode_openai_responses_request(
        &request_expr(),
        &OpenAiRequestOptions::new("gpt-5-mini", true, true),
    )
    .unwrap();
    assert_eq!(body, trim_newline(REQUEST));
    let decoded =
        decode_openai_responses_request(Input::Text(String::from_utf8(body).unwrap())).unwrap();
    validate_chat_transcript(&decoded).unwrap();
    assert_eq!(
        map_field(&decoded, "task"),
        Some(&Expr::String("summarize this file".into()))
    );
}

#[test]
fn responses_structured_output_and_open_params_are_projected() {
    let request = request_expr_with_extra(vec![
        key(
            "output-grammar",
            Expr::String(r#"{"type":"object","required":["answer"]}"#.into()),
        ),
        key(
            "output-grammar-dialect",
            Expr::Symbol(Symbol::new("json-schema")),
        ),
        key("output-grammar-required", Expr::Bool(true)),
        key(
            "bridge-calls",
            Expr::Vector(vec![Expr::Map(vec![key(
                "model-params",
                Expr::Map(vec![
                    key("temperature", Expr::Nil),
                    key("top-p", Expr::String("0.9".into())),
                ]),
            )])]),
        ),
    ]);
    let body = encode_openai_responses_request(
        &request,
        &OpenAiRequestOptions::new("gpt-5-mini", false, false),
    )
    .unwrap();
    let actual: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let expected: serde_json::Value = serde_json::from_slice(STRUCTURED).unwrap();
    assert_eq!(actual, expected);
    assert!(actual.get("tools").is_none());
    assert!(actual["temperature"].is_null());
}

#[test]
fn responses_function_tools_are_flattened_to_the_native_wire() {
    let request = request_expr_with_extra(vec![key(
        "tools",
        Expr::List(vec![Expr::Map(vec![
            key("type", Expr::Symbol(Symbol::new("function"))),
            key(
                "function",
                Expr::Map(vec![
                    key("name", Expr::String("inspect".into())),
                    key("description", Expr::String("Inspect a file".into())),
                    key(
                        "parameters",
                        Expr::Map(vec![key("type", Expr::String("object".into()))]),
                    ),
                ]),
            ),
        ])]),
    )]);
    let body = encode_openai_responses_request(
        &request,
        &OpenAiRequestOptions::new("gpt-5-mini", false, true),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["tools"][0]["type"], "function");
    assert_eq!(json["tools"][0]["name"], "inspect");
    assert_eq!(json["tools"][0]["parameters"]["type"], "object");
    assert!(json["tools"][0].get("function").is_none());
}

#[test]
fn responses_response_stream_refusal_and_tool_call_are_canonical() {
    for body in [RESPONSE, REFUSAL] {
        let expr = decode_openai_responses_response(Symbol::new("remote"), "fallback", body, true)
            .unwrap();
        validate_chat_transcript(&expr).unwrap();
    }
    let response =
        decode_openai_responses_response(Symbol::new("remote"), "fallback", RESPONSE, false)
            .unwrap();
    let rendered = format!("{response:?}");
    assert!(rendered.contains("provider-request-id"));
    assert!(rendered.contains("tool-call"));
    assert!(rendered.contains("input-tokens"));

    let streamed =
        decode_openai_responses_stream(Symbol::new("remote"), "fallback", STREAM, true).unwrap();
    validate_chat_transcript(&streamed).unwrap();
    assert!(format!("{streamed:?}").contains("raw-provider-response"));
}

#[test]
fn responses_malformed_truncated_and_bounded_inputs_fail_closed() {
    for body in [MALFORMED, TRUNCATED] {
        assert!(
            decode_openai_responses_stream(Symbol::new("remote"), "model", body, false).is_err()
        );
    }
    let err = decode_openai_responses_response_with_limits(
        Symbol::new("remote"),
        "model",
        RESPONSE,
        false,
        DecodeLimits {
            max_string_bytes: 3,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, Error::CodecError { ref message, .. } if message.contains("string bytes"))
    );
    let nested = br#"{"status":"completed","output":[{"type":"function_call","call_id":"c","name":"f","arguments":"{\"a\":{\"b\":1}}"}]}"#;
    let err = decode_openai_responses_response_with_limits(
        Symbol::new("remote"),
        "model",
        nested,
        false,
        DecodeLimits {
            max_depth: 1,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, Error::CodecError { ref message, .. } if message.contains("recursion depth"))
    );
}

#[test]
fn chat_and_responses_wires_differ_but_decode_equivalently() {
    let options = OpenAiRequestOptions::new("gpt-5-mini", false, false);
    let chat = encode_openai_request(&request_expr(), &options).unwrap();
    let responses = encode_openai_responses_request(&request_expr(), &options).unwrap();
    assert_ne!(chat, responses);

    let chat_response = decode_openai_response(
        Symbol::new("remote"), "gpt-5-mini",
        br#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"compiled"}}],"usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}}"#,
        false,
    ).unwrap();
    let responses_response = decode_openai_responses_response(
        Symbol::new("remote"), "gpt-5-mini",
        br#"{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"compiled"}]}],"usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15}}"#,
        false,
    ).unwrap();
    assert!(chat_response.canonical_eq(&responses_response));
}

#[test]
fn existing_chat_request_bytes_remain_golden() {
    let bytes = encode_openai_request(
        &request_expr(),
        &OpenAiRequestOptions::new("gpt-5-mini", true, true),
    )
    .unwrap();
    assert_eq!(bytes, br#"{"model":"gpt-5-mini","stream":true,"messages":[{"role":"system","content":[{"type":"text","text":"Answer in precise prose."}]},{"role":"user","content":[{"type":"text","text":"Summarize src/lib.rs"}]},{"role":"user","content":[{"type":"text","text":"summarize this file"}]}],"tools":[],"stream_options":{"include_usage":true}}"#);
}

fn trim_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}
fn key(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}
