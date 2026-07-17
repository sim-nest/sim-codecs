use serde_json::Value;
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{Error, Expr, Result, Symbol};

use crate::{model_response_expr, text_part};

use super::super::ANTHROPIC_CODEC_ID;
use super::super::common::provider_symbol;
use super::shared::{
    error_message, error_response_expr, raw_content_part, raw_provider_expr, tool_call_part,
    usage_expr_from_value,
};

/// Decodes an Anthropic Messages response body into a model-response
/// transcript, optionally embedding the raw provider JSON.
pub fn decode_anthropic_response(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    decode_anthropic_response_with_limits(runner, model, body, include_raw, DecodeLimits::default())
}

/// Decodes an Anthropic Messages response body under caller-supplied limits.
pub fn decode_anthropic_response_with_limits(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
    limits: DecodeLimits,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(limits);
    budget.check_input_bytes(ANTHROPIC_CODEC_ID, body.len())?;
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| Error::Eval(format!("anthropic codec returned invalid json: {err}")))?;
    response_expr_from_json(runner, model, &value, include_raw, &mut budget)
}

fn response_expr_from_json(
    runner: Symbol,
    model: &str,
    value: &Value,
    include_raw: bool,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let response = value
        .as_object()
        .ok_or_else(|| Error::Eval("anthropic response must be a json object".to_owned()))?;
    if let Some(error) = error_message(Some(response)) {
        return error_response_expr(runner, model, error, include_raw, Some(value), budget);
    }
    let content = response
        .get("content")
        .ok_or_else(|| Error::Eval("anthropic response missing content".to_owned()))?;
    let stop_reason = response
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let response_model = response
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(model);
    let mut entries = match model_response_expr(
        runner,
        response_model,
        response_content_parts(content, budget)?,
        provider_symbol(stop_reason),
    ) {
        Expr::Map(entries) => entries,
        _ => unreachable!("model_response_expr always returns a map"),
    };
    if let Some(usage) = usage_expr_from_value(value)? {
        entries.push((Expr::Symbol(Symbol::new("usage")), usage));
    }
    if include_raw {
        entries.push((
            Expr::Symbol(Symbol::new("raw-provider-response")),
            raw_provider_expr(value, budget)?,
        ));
    }
    Ok(Expr::Map(entries))
}

fn response_content_parts(value: &Value, budget: &mut DecodeBudget) -> Result<Vec<Expr>> {
    match value {
        Value::Array(parts) => parts
            .iter()
            .map(|part| response_part_from_json(part, budget))
            .collect(),
        other => Err(Error::Eval(format!(
            "anthropic response content must be an array, found {other:?}"
        ))),
    }
}

fn response_part_from_json(value: &Value, budget: &mut DecodeBudget) -> Result<Expr> {
    let object = value.as_object().ok_or_else(|| {
        Error::Eval("anthropic response content part must be an object".to_owned())
    })?;
    match object.get("type").and_then(Value::as_str).unwrap_or("text") {
        "text" => Ok(text_part(
            object.get("text").and_then(Value::as_str).ok_or_else(|| {
                Error::Eval("anthropic response text part missing text".to_owned())
            })?,
        )),
        "tool_use" | "server_tool_use" => tool_call_part(ANTHROPIC_CODEC_ID, object, budget),
        other => raw_content_part(ANTHROPIC_CODEC_ID, other, value, budget),
    }
}
