use sim_kernel::{Expr, Symbol};

use crate::{
    AnthropicRequestOptions, OpenAiRequestOptions, encode_anthropic_request, encode_openai_request,
};

use super::request_expr_with_extra;

#[test]
fn openai_request_encoder_attaches_json_schema_output_grammar() {
    let body = encode_openai_request(
        &request_expr_with_extra(vec![
            (
                Expr::Symbol(Symbol::new("output-grammar")),
                Expr::String(r#"{"type":"string"}"#.to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("output-grammar-dialect")),
                Expr::Symbol(Symbol::new("json-schema")),
            ),
            (
                Expr::Symbol(Symbol::new("output-grammar-required")),
                Expr::Bool(true),
            ),
        ]),
        &OpenAiRequestOptions::new("gpt-5-mini", false, false),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["response_format"]["type"], "json_schema");
    assert_eq!(
        json["response_format"]["json_schema"]["schema"]["type"],
        "string"
    );
    assert_eq!(json["response_format"]["json_schema"]["strict"], true);
}

#[test]
fn openai_request_encoder_renders_json_schema_return_shape() {
    let body = encode_openai_request(
        &request_expr_with_extra(vec![
            (
                Expr::Symbol(Symbol::new("return-codec")),
                Expr::Symbol(Symbol::qualified("codec", "json")),
            ),
            (
                Expr::Symbol(Symbol::new("return-shape")),
                Expr::Symbol(Symbol::new("String")),
            ),
            (
                Expr::Symbol(Symbol::new("output-grammar-dialect")),
                Expr::Symbol(Symbol::new("json-schema")),
            ),
        ]),
        &OpenAiRequestOptions::new("gpt-5-mini", false, false),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["response_format"]["type"], "json_schema");
    assert_eq!(
        json["response_format"]["json_schema"]["schema"]["$comment"],
        "codec/json position=data target=datum"
    );
}

#[test]
fn openai_request_encoder_rejects_unsupported_output_grammar() {
    let err = encode_openai_request(
        &request_expr_with_extra(vec![
            (
                Expr::Symbol(Symbol::new("output-grammar")),
                Expr::String("root ::= string".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("output-grammar-dialect")),
                Expr::Symbol(Symbol::new("gbnf")),
            ),
        ]),
        &OpenAiRequestOptions::new("gpt-5-mini", false, false),
    )
    .unwrap_err();

    assert!(format!("{err:?}").contains("Gbnf"));
}

#[test]
fn openai_request_encoder_attaches_bridge_model_params() {
    let body = encode_openai_request(
        &request_expr_with_extra(vec![bridge_calls_model_params()]),
        &OpenAiRequestOptions::new("gpt-5-mini", false, false),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["temperature"], 0);
    assert_eq!(json["top_p"], 1);
}

#[test]
fn anthropic_request_encoder_attaches_bridge_model_params() {
    let body = encode_anthropic_request(
        &request_expr_with_extra(vec![bridge_calls_model_params()]),
        &AnthropicRequestOptions::new("claude-sonnet-latest", 512, false, false),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["temperature"], 0);
    assert_eq!(json["top_p"], 1);
}

#[test]
fn openai_request_encoder_rejects_structural_bridge_model_param_override() {
    let err = encode_openai_request(
        &request_expr_with_extra(vec![(
            Expr::Symbol(Symbol::new("bridge-calls")),
            Expr::Vector(vec![Expr::Map(vec![(
                Expr::Symbol(Symbol::new("model-params")),
                Expr::Map(vec![(
                    Expr::Symbol(Symbol::new("messages")),
                    Expr::String("bad".to_owned()),
                )]),
            )])]),
        )]),
        &OpenAiRequestOptions::new("gpt-5-mini", false, false),
    )
    .unwrap_err();

    assert!(format!("{err:?}").contains("cannot override provider request field"));
}

fn bridge_calls_model_params() -> (Expr, Expr) {
    (
        Expr::Symbol(Symbol::new("bridge-calls")),
        Expr::Vector(vec![Expr::Map(vec![(
            Expr::Symbol(Symbol::new("model-params")),
            Expr::Map(vec![
                (
                    Expr::Symbol(Symbol::new("temperature")),
                    Expr::String("0".to_owned()),
                ),
                (
                    Expr::Symbol(Symbol::new("top-p")),
                    Expr::String("1".to_owned()),
                ),
            ]),
        )])]),
    )
}
