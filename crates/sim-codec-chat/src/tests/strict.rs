use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Error, Expr, ReadPolicy, Symbol};

use super::{cx, message_expr};

#[test]
fn encoder_rejects_duplicate_marker_fields() {
    let mut cx = cx();
    let expr = Expr::Map(vec![
        key_bool("model-request", true),
        key_bool("model-request", false),
        key_expr("task", Expr::String("summarize".to_owned())),
        key_expr("messages", Expr::List(Vec::new())),
    ]);

    let err =
        encode_with_codec(&mut cx, &chat_codec(), &expr, EncodeOptions::default()).unwrap_err();

    assert_eval_contains(err, "duplicate model-request field");
}

#[test]
fn decoder_rejects_duplicate_required_fields() {
    let mut cx = cx();
    let expr = Expr::Map(vec![
        key_bool("model-request", true),
        key_expr("task", Expr::String("summarize".to_owned())),
        key_expr("messages", Expr::List(Vec::new())),
        key_expr("messages", Expr::List(vec![message_expr("user", "hello")])),
    ]);
    let text = crate::expr::encode_chat_text(&expr);

    let err = decode_with_codec(
        &mut cx,
        &chat_codec(),
        Input::Text(text),
        ReadPolicy::default(),
    )
    .unwrap_err();

    assert_eval_contains(err, "duplicate messages field");
}

#[test]
fn validator_rejects_duplicate_nested_content_fields() {
    let expr = response_with_content(Expr::Map(vec![
        key_expr("type", Expr::Symbol(Symbol::new("text"))),
        key_expr("text", Expr::String("one".to_owned())),
        key_expr("text", Expr::String("two".to_owned())),
    ]));

    let err = crate::validate_chat_transcript(&expr).unwrap_err();

    assert_eval_contains(err, "duplicate text field");
}

#[test]
fn validator_rejects_duplicate_tool_call_fields() {
    let expr = response_with_content(Expr::Map(vec![
        key_expr("type", Expr::Symbol(Symbol::new("tool-call"))),
        key_expr("id", Expr::String("call-1".to_owned())),
        key_expr("id", Expr::String("call-2".to_owned())),
        key_expr("name", Expr::String("lookup".to_owned())),
        key_expr("arguments", Expr::Map(Vec::new())),
    ]));

    let err = crate::validate_chat_transcript(&expr).unwrap_err();

    assert_eval_contains(err, "duplicate id field");
}

#[test]
fn qualified_extension_fields_do_not_conflict_with_owned_fields() {
    let mut cx = cx();
    let expr = Expr::Map(vec![
        key_bool("model-request", true),
        key_expr("task", Expr::String("summarize".to_owned())),
        key_expr("messages", Expr::List(vec![message_expr("user", "hello")])),
        (
            Expr::Symbol(Symbol::qualified("vendor", "messages")),
            Expr::String("metadata-a".to_owned()),
        ),
        (
            Expr::Symbol(Symbol::qualified("vendor", "messages")),
            Expr::String("metadata-b".to_owned()),
        ),
    ]);

    crate::validate_chat_transcript(&expr).unwrap();
    let text = encode_with_codec(&mut cx, &chat_codec(), &expr, EncodeOptions::default())
        .unwrap()
        .into_text()
        .unwrap();
    let decoded = decode_with_codec(
        &mut cx,
        &chat_codec(),
        Input::Text(text),
        ReadPolicy::default(),
    )
    .unwrap();

    crate::validate_chat_transcript(&decoded).unwrap();
}

#[test]
fn model_card_advisory_fields_remain_extension_space() {
    let mut card = match crate::model_card_expr(
        Symbol::new("runner"),
        "fake/model",
        Symbol::new("fake"),
        Symbol::new("local"),
    ) {
        Expr::Map(entries) => entries,
        other => panic!("expected model card map, found {other:?}"),
    };
    card.push(key_expr("supports-shape", Expr::Bool(true)));

    crate::validate_chat_transcript(&Expr::Map(card)).unwrap();
}

fn response_with_content(part: Expr) -> Expr {
    crate::model_response_expr(
        Symbol::new("runner"),
        "model",
        vec![part],
        Symbol::new("stop"),
    )
}

fn chat_codec() -> Symbol {
    Symbol::qualified("codec", "chat")
}

fn key_bool(name: &str, value: bool) -> (Expr, Expr) {
    key_expr(name, Expr::Bool(value))
}

fn key_expr(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}

fn assert_eval_contains(err: Error, needle: &str) {
    match err {
        Error::Eval(message) => assert!(
            message.contains(needle),
            "expected {needle:?} in {message:?}"
        ),
        other => panic!("expected Eval error, found {other:?}"),
    }
}
