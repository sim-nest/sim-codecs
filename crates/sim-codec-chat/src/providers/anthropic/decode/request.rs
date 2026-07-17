use serde_json::{Map, Value};
use sim_codec::{DecodeBudget, DecodeLimits, Input, domain_input_text};
use sim_codec_json::{JsonProjectionMode, project_json_to_expr_budgeted};
use sim_kernel::{CodecId, Expr, Result, Symbol};

use crate::{text_part, validate_chat_transcript};

use super::super::ANTHROPIC_CODEC_ID;
use super::super::common::{codec_error, codec_eval_to_codec};
use super::shared::{raw_content_part, string_member, tool_call_part, tool_result_part};

/// Decodes Anthropic request JSON into a validated chat-transcript expression.
pub fn decode_anthropic_request(input: Input) -> Result<Expr> {
    decode_anthropic_request_with_limits(input, DecodeLimits::default())
}

/// Decodes Anthropic request JSON under caller-supplied decode limits.
pub fn decode_anthropic_request_with_limits(input: Input, limits: DecodeLimits) -> Result<Expr> {
    decode_anthropic_request_for_codec_with_limits(ANTHROPIC_CODEC_ID, input, limits)
}

pub(in crate::providers::anthropic) fn decode_anthropic_request_for_codec_with_limits(
    codec: CodecId,
    input: Input,
    limits: DecodeLimits,
) -> Result<Expr> {
    let source = domain_input_text(codec, input)?;
    let mut budget = DecodeBudget::new(limits);
    budget.check_input_bytes(codec, source.len())?;
    let value = serde_json::from_str::<Value>(&source).map_err(|err| codec_error(codec, err))?;
    let request = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "anthropic request must be a json object"))?;
    let model = string_member(codec, request, "model")?.to_owned();
    let (task, messages) = request_task_and_messages(codec, request, &mut budget)?;
    let mut entries = vec![
        (Expr::Symbol(Symbol::new("model-request")), Expr::Bool(true)),
        (Expr::Symbol(Symbol::new("task")), task),
        (Expr::Symbol(Symbol::new("messages")), Expr::List(messages)),
        (Expr::Symbol(Symbol::new("model")), Expr::String(model)),
    ];
    if let Some(stream) = request.get("stream").and_then(Value::as_bool) {
        entries.push((Expr::Symbol(Symbol::new("stream")), Expr::Bool(stream)));
    }
    push_projected_field(
        codec,
        &mut budget,
        request,
        &mut entries,
        "max_tokens",
        "max-tokens",
    )?;
    push_projected_field(codec, &mut budget, request, &mut entries, "tools", "tools")?;
    push_projected_field(
        codec,
        &mut budget,
        request,
        &mut entries,
        "tool_choice",
        "tool-choice",
    )?;
    let expr = Expr::Map(entries);
    validate_chat_transcript(&expr).map_err(|err| codec_eval_to_codec(codec, err))?;
    Ok(expr)
}

fn push_projected_field(
    codec: CodecId,
    budget: &mut DecodeBudget,
    request: &Map<String, Value>,
    entries: &mut Vec<(Expr, Expr)>,
    provider_key: &str,
    transcript_key: &str,
) -> Result<()> {
    let Some(value) = request
        .get(provider_key)
        .or_else(|| request.get(transcript_key))
    else {
        return Ok(());
    };
    entries.push((
        Expr::Symbol(Symbol::new(transcript_key)),
        project_json_to_expr_budgeted(
            value,
            JsonProjectionMode::UntaggedInterop,
            codec,
            budget,
            0,
        )?,
    ));
    Ok(())
}

fn request_task_and_messages(
    codec: CodecId,
    request: &Map<String, Value>,
    budget: &mut DecodeBudget,
) -> Result<(Expr, Vec<Expr>)> {
    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| codec_error(codec, "anthropic request missing messages"))?;
    let (task_message, prior_messages) = messages
        .split_last()
        .ok_or_else(|| codec_error(codec, "anthropic request messages must not be empty"))?;
    let mut transcript_messages = Vec::new();
    if let Some(system) = request.get("system") {
        transcript_messages.push(system_message_expr(codec, system, budget)?);
    }
    transcript_messages.extend(
        prior_messages
            .iter()
            .map(|message| message_expr(codec, message, budget))
            .collect::<Result<Vec<_>>>()?,
    );
    Ok((
        Expr::String(message_text(codec, task_message)?),
        transcript_messages,
    ))
}

fn system_message_expr(codec: CodecId, value: &Value, budget: &mut DecodeBudget) -> Result<Expr> {
    Ok(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("role")),
            Expr::Symbol(Symbol::new("system")),
        ),
        (
            Expr::Symbol(Symbol::new("content")),
            Expr::List(content_parts(codec, Some(value), budget)?),
        ),
    ]))
}

fn message_expr(codec: CodecId, value: &Value, budget: &mut DecodeBudget) -> Result<Expr> {
    let object = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "anthropic message must be an object"))?;
    let role = string_member(codec, object, "role")?;
    Ok(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("role")),
            Expr::Symbol(Symbol::new(role)),
        ),
        (
            Expr::Symbol(Symbol::new("content")),
            Expr::List(content_parts(codec, object.get("content"), budget)?),
        ),
    ]))
}

fn message_text(codec: CodecId, value: &Value) -> Result<String> {
    let object = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "anthropic task message must be an object"))?;
    let role = string_member(codec, object, "role")?;
    if role != "user" {
        return Err(codec_error(
            codec,
            "anthropic request final message must have role user",
        ));
    }
    text_from_content(codec, object.get("content"))
}

fn text_from_content(codec: CodecId, content: Option<&Value>) -> Result<String> {
    match content {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    part.as_object().and_then(|object| {
                        (object.get("type").and_then(Value::as_str) == Some("text"))
                            .then(|| object.get("text").and_then(Value::as_str).unwrap_or(""))
                    })
                })
                .map(str::to_owned)
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text)
        }
        Some(Value::Null) | None => Ok(String::new()),
        _ => Err(codec_error(
            codec,
            "anthropic message content must be string, array, or null",
        )),
    }
}

fn content_parts(
    codec: CodecId,
    content: Option<&Value>,
    budget: &mut DecodeBudget,
) -> Result<Vec<Expr>> {
    match content {
        Some(Value::String(text)) => Ok(vec![text_part(text)]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| request_part_from_json(codec, part, budget))
            .collect(),
        Some(Value::Null) | None => Ok(Vec::new()),
        _ => Err(codec_error(
            codec,
            "anthropic message content must be string, array, or null",
        )),
    }
}

fn request_part_from_json(
    codec: CodecId,
    value: &Value,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let object = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "anthropic content part must be an object"))?;
    match object.get("type").and_then(Value::as_str).unwrap_or("text") {
        "text" => Ok(text_part(
            object
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| codec_error(codec, "anthropic text content part missing text"))?,
        )),
        "tool_use" | "server_tool_use" => tool_call_part(codec, object, budget),
        "tool_result" => tool_result_part(codec, object, budget),
        other => raw_content_part(codec, other, value, budget),
    }
}
