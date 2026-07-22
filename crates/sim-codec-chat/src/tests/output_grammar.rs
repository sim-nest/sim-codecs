use sim_kernel::{Expr, Symbol};

use crate::{OpenAiRequestOptions, encode_openai_request};

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
