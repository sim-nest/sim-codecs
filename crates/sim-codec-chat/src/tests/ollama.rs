use sim_kernel::{Error, Expr, Symbol};

use crate::{
    OllamaRequestOptions, decode_ollama_response, decode_ollama_stream, encode_ollama_request,
};

use super::request_expr;

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
fn ollama_request_reads_namespace_agnostic_provider_fields() {
    // The ollama request readers are the namespace-agnostic `_any` family: a
    // provider content part may spell its `text` field with a string key rather
    // than a bare symbol, and the encoder must still read it. This pins the
    // intended key-agnostic behavior (a bare-symbol OR string provider key) that
    // motivates the `entry_required_*_any` substrate variants over the strict
    // bare-symbol readers.
    let request = Expr::Map(vec![
        (Expr::Symbol(Symbol::new("model-request")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("task")),
            Expr::String("summarize".to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("messages")),
            Expr::List(vec![Expr::Map(vec![
                (
                    Expr::Symbol(Symbol::new("role")),
                    Expr::Symbol(Symbol::new("user")),
                ),
                (
                    Expr::Symbol(Symbol::new("content")),
                    Expr::List(vec![Expr::Map(vec![
                        (
                            Expr::Symbol(Symbol::new("type")),
                            Expr::Symbol(Symbol::new("text")),
                        ),
                        (
                            Expr::String("text".to_owned()),
                            Expr::String("string keyed body".to_owned()),
                        ),
                    ])]),
                ),
            ])]),
        ),
    ]);
    let body = encode_ollama_request(
        &request,
        &OllamaRequestOptions::new("qwen3.5:4b", false, false),
    )
    .unwrap();
    let text = String::from_utf8(body).unwrap();
    assert!(text.contains("string keyed body"), "{text}");
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
    // The raw-provider-response projection runs under a decode budget; a
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
fn ollama_usage_token_total_saturates_without_overflow() {
    // prompt_eval_count + eval_count is a u64 add over attacker-controlled
    // numbers; it must saturate instead of overflowing and panicking.
    let body = format!(
        r#"{{"response":"ok","done":true,"prompt_eval_count":{max},"eval_count":{max}}}"#,
        max = u64::MAX
    );
    let expr = decode_ollama_response(Symbol::new("local"), "m", body.as_bytes(), false).unwrap();
    assert!(format!("{expr:?}").contains(&u64::MAX.to_string()));
}
