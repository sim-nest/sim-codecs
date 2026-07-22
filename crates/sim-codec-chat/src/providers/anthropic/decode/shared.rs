use serde_json::{Map, Value};
use sim_codec::DecodeBudget;
use sim_codec_json::{JsonProjectionMode, json_number_to_u64, project_json_to_expr_budgeted};
use sim_kernel::{CodecId, Expr, Result, Symbol};

use crate::{model_error_expr, usage_record};

use super::super::ANTHROPIC_CODEC_ID;
use super::super::common::{codec_error, provider_symbol};

pub(in crate::providers::anthropic::decode) fn tool_call_part(
    codec: CodecId,
    object: &Map<String, Value>,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    Ok(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("type")),
            Expr::Symbol(Symbol::new("tool-call")),
        ),
        (
            Expr::Symbol(Symbol::new("id")),
            Expr::String(string_member(codec, object, "id")?.to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("name")),
            Expr::String(string_member(codec, object, "name")?.to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("arguments")),
            project_json_to_expr_budgeted(
                object.get("input").unwrap_or(&Value::Object(Map::new())),
                JsonProjectionMode::UntaggedInterop,
                codec,
                budget,
                0,
            )?,
        ),
    ]))
}

pub(in crate::providers::anthropic::decode) fn tool_result_part(
    codec: CodecId,
    object: &Map<String, Value>,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let output = object
        .get("content")
        .map(|content| {
            project_json_to_expr_budgeted(
                content,
                JsonProjectionMode::UntaggedInterop,
                codec,
                budget,
                0,
            )
        })
        .transpose()?
        .unwrap_or(Expr::Nil);
    let status = if object
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "error"
    } else {
        "ok"
    };
    Ok(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("type")),
            Expr::Symbol(Symbol::new("tool-result")),
        ),
        (
            Expr::Symbol(Symbol::new("tool-call-id")),
            Expr::String(string_member(codec, object, "tool_use_id")?.to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("status")),
            Expr::Symbol(Symbol::new(status)),
        ),
        (Expr::Symbol(Symbol::new("output")), output),
    ]))
}

pub(in crate::providers::anthropic::decode) fn raw_content_part(
    codec: CodecId,
    kind: &str,
    value: &Value,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    Ok(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("type")),
            Expr::Symbol(provider_symbol(kind)),
        ),
        (
            Expr::Symbol(Symbol::new("raw-provider-part")),
            project_json_to_expr_budgeted(
                value,
                JsonProjectionMode::UntaggedInterop,
                codec,
                budget,
                0,
            )?,
        ),
    ]))
}

pub(in crate::providers::anthropic::decode) fn error_response_expr(
    runner: Symbol,
    model: &str,
    message: String,
    include_raw: bool,
    raw: Option<&Value>,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let mut entries = match model_error_expr(runner, model, message) {
        Expr::Map(entries) => entries,
        _ => unreachable!("model_error_expr always returns a map"),
    };
    if include_raw && let Some(raw) = raw {
        entries.push((
            Expr::Symbol(Symbol::new("raw-provider-response")),
            raw_provider_expr(raw, budget)?,
        ));
    }
    Ok(Expr::Map(entries))
}

pub(in crate::providers::anthropic::decode) fn usage_expr_from_value(
    value: &Value,
) -> Result<Option<Expr>> {
    let Some(usage) = value.get("usage").and_then(Value::as_object) else {
        return Ok(None);
    };
    let input = usage.get("input_tokens").and_then(json_number_to_u64);
    let output = usage.get("output_tokens").and_then(json_number_to_u64);
    let total = input
        .zip(output)
        .map(|(input, output)| input.saturating_add(output));
    let fields = usage_record(input, output, total);
    Ok((!fields.is_empty()).then_some(Expr::Map(fields)))
}

pub(in crate::providers::anthropic::decode) fn raw_provider_expr(
    value: &Value,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    project_json_to_expr_budgeted(
        value,
        JsonProjectionMode::UntaggedInterop,
        ANTHROPIC_CODEC_ID,
        budget,
        0,
    )
}

pub(in crate::providers::anthropic::decode) fn error_message(
    object: Option<&Map<String, Value>>,
) -> Option<String> {
    let error = object?.get("error")?;
    match error {
        Value::String(message) => Some(message.clone()),
        Value::Object(fields) => fields
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                fields
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
        _ => None,
    }
}

pub(in crate::providers::anthropic::decode) fn model_event_expr(
    event: &str,
    runner: Symbol,
    model: &str,
    span_id: Expr,
    extra: Vec<(Expr, Expr)>,
) -> Expr {
    let mut entries = vec![
        (Expr::Symbol(Symbol::new("model-event")), Expr::Bool(true)),
        (
            Expr::Symbol(Symbol::new("event")),
            Expr::Symbol(Symbol::new(event)),
        ),
        (Expr::Symbol(Symbol::new("runner")), Expr::Symbol(runner)),
        (
            Expr::Symbol(Symbol::new("model")),
            Expr::String(model.to_owned()),
        ),
        (Expr::Symbol(Symbol::new("span-id")), span_id),
    ];
    entries.extend(extra);
    Expr::Map(entries)
}

pub(in crate::providers::anthropic::decode) fn final_event_expr(
    runner: Symbol,
    model: &str,
    span_id: Expr,
    response: Expr,
) -> Expr {
    model_event_expr(
        "final",
        runner,
        model,
        span_id,
        vec![(Expr::Symbol(Symbol::new("response")), response)],
    )
}

pub(in crate::providers::anthropic::decode) fn event_response(event: &Expr) -> Option<&Expr> {
    let Expr::Map(entries) = event else {
        return None;
    };
    entries.iter().find_map(|(key, value)| match key {
        Expr::Symbol(symbol) if symbol.name.as_ref() == "response" => Some(value),
        _ => None,
    })
}

pub(in crate::providers::anthropic::decode) fn string_member<'a>(
    codec: CodecId,
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| codec_error(codec, format!("anthropic missing string {name}")))
}
