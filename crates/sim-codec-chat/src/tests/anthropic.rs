use sim_codec::{DecodeLimits, Input, decode_with_codec, decode_with_codec_and_limits};
use sim_kernel::{Error, Expr, ReadPolicy, Symbol};

use crate::{
    AnthropicRequestOptions, decode_anthropic_response, decode_anthropic_response_with_limits,
    decode_anthropic_stream, decode_anthropic_stream_events, decode_anthropic_stream_with_limits,
    encode_anthropic_request,
};

use super::{cx, message_expr, request_expr_with_extra};

#[test]
fn anthropic_runtime_codec_decodes_messages_request() {
    let mut cx = cx();
    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "anthropic"),
        Input::Text(
            r#"{"model":"claude-sonnet-4-20250514","max_tokens":64,"system":"Answer briefly.","messages":[{"role":"user","content":"hello"}]}"#
                .to_owned(),
        ),
        ReadPolicy::default(),
    )
    .unwrap();

    crate::validate_chat_transcript(&decoded).unwrap();
    assert!(format!("{decoded:?}").contains("model-request"));
    assert!(format!("{decoded:?}").contains("Answer briefly."));
    assert!(format!("{decoded:?}").contains("hello"));
}

#[test]
fn anthropic_runtime_request_honors_decode_input_limit() {
    let mut cx = cx();
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "anthropic"),
        Input::Text(
            r#"{"model":"claude-sonnet-4-20250514","max_tokens":64,"messages":[{"role":"user","content":"hello"}]}"#
                .to_owned(),
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
fn anthropic_request_encoder_splits_system_and_tool_schema() {
    let request = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("model-request")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("task")),
            Expr::String("check the weather".to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("messages")),
            Expr::List(vec![
                message_expr("system", "Be concise."),
                message_expr("user", "Use the weather tool."),
            ]),
        ),
        (
            Expr::Symbol(Symbol::new("tools")),
            Expr::List(vec![Expr::Map(vec![
                (
                    Expr::Symbol(Symbol::new("name")),
                    Expr::String("get_weather".to_owned()),
                ),
                (
                    Expr::Symbol(Symbol::new("description")),
                    Expr::String("Read a city forecast.".to_owned()),
                ),
                (
                    Expr::Symbol(Symbol::new("parameters")),
                    Expr::Map(vec![
                        (
                            Expr::Symbol(Symbol::new("type")),
                            Expr::String("object".to_owned()),
                        ),
                        (
                            Expr::Symbol(Symbol::new("properties")),
                            Expr::Map(vec![(
                                Expr::Symbol(Symbol::new("location")),
                                Expr::Map(vec![(
                                    Expr::Symbol(Symbol::new("type")),
                                    Expr::String("string".to_owned()),
                                )]),
                            )]),
                        ),
                    ]),
                ),
            ])]),
        ),
    ]);
    let body = encode_anthropic_request(
        &request,
        &AnthropicRequestOptions::new("claude-sonnet-4-20250514", 256, true, true),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["model"], "claude-sonnet-4-20250514");
    assert_eq!(json["max_tokens"], 256);
    assert_eq!(json["stream"], true);
    assert_eq!(json["system"], "Be concise.");
    assert_eq!(json["messages"][0]["role"], "user");
    assert_eq!(
        json["messages"][1]["content"][0]["text"],
        "check the weather"
    );
    assert_eq!(json["tools"][0]["name"], "get_weather");
    assert_eq!(
        json["tools"][0]["input_schema"]["properties"]["location"]["type"],
        "string"
    );
}

#[test]
fn anthropic_request_encoder_rejects_output_grammar() {
    let err = encode_anthropic_request(
        &request_expr_with_extra(vec![(
            Expr::Symbol(Symbol::new("output-grammar")),
            Expr::String(r#"{"type":"string"}"#.to_owned()),
        )]),
        &AnthropicRequestOptions::new("claude-sonnet-4-20250514", 256, false, false),
    )
    .unwrap_err();

    assert!(format!("{err:?}").contains("does not support output grammar"));
}

#[test]
fn anthropic_messages_response_decodes() {
    let body = br#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":1}}"#;
    let expr = decode_anthropic_response(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        body,
        true,
    )
    .unwrap();

    crate::validate_chat_transcript(&expr).unwrap();
    assert!(format!("{expr:?}").contains("ok"));
    assert!(format!("{expr:?}").contains("end-turn"));
    assert!(format!("{expr:?}").contains("input-tokens"));
    assert!(format!("{expr:?}").contains("raw-provider-response"));
}

#[test]
fn anthropic_response_raw_projection_honors_decode_collection_limit() {
    let body = br#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}"#;
    let err = decode_anthropic_response_with_limits(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        body,
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
fn anthropic_tool_use_response_decodes_to_tool_call_part() {
    let body = br#"{"id":"msg_tool","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{"location":"Stockholm"}}],"stop_reason":"tool_use","usage":{"input_tokens":9,"output_tokens":4}}"#;
    let expr = decode_anthropic_response(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        body,
        false,
    )
    .unwrap();

    crate::validate_chat_transcript(&expr).unwrap();
    assert!(format!("{expr:?}").contains("tool-call"));
    assert!(format!("{expr:?}").contains("toolu_1"));
    assert!(format!("{expr:?}").contains("Stockholm"));
    assert!(format!("{expr:?}").contains("tool-use"));
}

#[test]
fn anthropic_stream_decoder_emits_events_and_final_response() {
    let body = br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[]}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello "}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":2}}

event: message_stop
data: {"type":"message_stop"}
"#;
    let events = decode_anthropic_stream_events(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        body,
        true,
    )
    .unwrap();
    assert!(
        events
            .iter()
            .any(|event| format!("{event:?}").contains("delta"))
    );
    assert!(
        events
            .iter()
            .any(|event| format!("{event:?}").contains("usage"))
    );
    let final_response = decode_anthropic_stream(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        body,
        true,
    )
    .unwrap();
    crate::validate_chat_transcript(&final_response).unwrap();
    assert!(format!("{final_response:?}").contains("hello world"));
    assert!(format!("{final_response:?}").contains("raw-provider-response"));
}

#[test]
fn anthropic_stream_honors_decode_input_limit() {
    let body = br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet-4-20250514"}}

event: message_stop
data: {"type":"message_stop"}
"#;
    let err = decode_anthropic_stream_with_limits(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        body,
        true,
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
fn anthropic_stream_event_accumulation_honors_decode_collection_limit() {
    let body = br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet-4-20250514"}}

event: message_stop
data: {"type":"message_stop"}
"#;
    let err = decode_anthropic_stream_with_limits(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        body,
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
fn anthropic_error_envelope_decodes_to_model_error() {
    let expr = decode_anthropic_response(
        Symbol::qualified("runner", "anthropic"),
        "claude-sonnet-4-20250514",
        br#"{"type":"error","error":{"type":"invalid_request_error","message":"bad key"}}"#,
        false,
    )
    .unwrap();

    crate::validate_chat_transcript(&expr).unwrap();
    assert!(format!("{expr:?}").contains("bad key"));
    assert!(format!("{expr:?}").contains("shape-ok"));
}
