use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Expr, ReadPolicy, Result, Symbol};

use crate::{
    OpenAiRequestOptions, decode_lemonade_response, decode_lemonade_stream,
    decode_lm_studio_response, decode_lm_studio_stream, encode_lemonade_request,
    encode_lm_studio_request,
};

use super::{cx, request_expr, response_expr};

#[test]
fn lm_studio_codec_uses_openai_wire_and_stamps_provider() {
    assert_openai_compatible_provider(
        "lm-studio",
        encode_lm_studio_request,
        decode_lm_studio_response,
        decode_lm_studio_stream,
    );
}

#[test]
fn lemonade_codec_uses_openai_wire_and_stamps_provider() {
    assert_openai_compatible_provider(
        "lemonade",
        encode_lemonade_request,
        decode_lemonade_response,
        decode_lemonade_stream,
    );
}

fn assert_openai_compatible_provider(
    provider: &'static str,
    encode_request: fn(&Expr, &OpenAiRequestOptions) -> Result<Vec<u8>>,
    decode_response: fn(Symbol, &str, &[u8], bool) -> Result<Expr>,
    decode_stream: fn(Symbol, &str, &[u8], bool) -> Result<Expr>,
) {
    let mut cx = cx();
    let codec = Symbol::qualified("codec", provider);
    let runner = Symbol::qualified("runner", provider);

    let decoded_request = decode_with_codec(
        &mut cx,
        &codec,
        Input::Text(
            r#"{"model":"local/model","messages":[{"role":"system","content":"stay local"},{"role":"user","content":"hello"}],"stream":false}"#
                .to_owned(),
        ),
        ReadPolicy::default(),
    )
    .unwrap();
    crate::validate_chat_transcript(&decoded_request).unwrap();
    assert_eq!(provider_name(&decoded_request), provider);

    let request_body = encode_request(
        &request_expr(),
        &OpenAiRequestOptions::new("local/model", true, true),
    )
    .unwrap();
    let request_json: serde_json::Value = serde_json::from_slice(&request_body).unwrap();
    assert_eq!(request_json["model"], "local/model");
    assert_eq!(request_json["stream"], true);
    assert_eq!(request_json["messages"][0]["role"], "system");
    assert_eq!(
        request_json["stream_options"]["include_usage"],
        serde_json::Value::Bool(true)
    );

    let encoded_response =
        encode_with_codec(&mut cx, &codec, &response_expr(), EncodeOptions::default())
            .unwrap()
            .into_text()
            .unwrap();
    assert!(encoded_response.contains("chat.completion"));

    let decoded_response = decode_response(
        runner.clone(),
        "local/model",
        encoded_response.as_bytes(),
        true,
    )
    .unwrap();
    crate::validate_chat_transcript(&decoded_response).unwrap();
    assert_eq!(provider_name(&decoded_response), provider);
    assert!(format!("{decoded_response:?}").contains("raw-provider-response"));

    let decoded_stream =
        decode_stream(runner, "local/model", openai_stream_fixture(), false).unwrap();
    crate::validate_chat_transcript(&decoded_stream).unwrap();
    assert_eq!(provider_name(&decoded_stream), provider);
    assert!(format!("{decoded_stream:?}").contains("hello world"));
}

fn provider_name(expr: &Expr) -> &str {
    let Expr::Map(entries) = expr else {
        panic!("expected transcript map");
    };
    entries
        .iter()
        .find_map(|(key, value)| match (key, value) {
            (Expr::Symbol(key), Expr::Symbol(provider)) if key.name.as_ref() == "provider" => {
                Some(provider.name.as_ref())
            }
            _ => None,
        })
        .expect("missing provider field")
}

fn openai_stream_fixture() -> &'static [u8] {
    br#"data: {"id":"chunk-1","choices":[{"delta":{"content":"hello "},"finish_reason":null}]}

data: {"id":"chunk-1","choices":[{"delta":{"content":"world"},"finish_reason":"stop"}]}

data: [DONE]
"#
}
