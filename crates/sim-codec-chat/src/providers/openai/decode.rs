use serde_json::{Map, Value};
use sim_codec::{DecodeBudget, DecodeLimits, Input, domain_input_text};
use sim_codec_json::{JsonProjectionMode, json_number_to_u64, project_json_to_expr_budgeted};
use sim_kernel::{CodecId, Error, Expr, Result, Symbol};

use crate::{
    model_error_expr, model_response_expr, text_part, usage_record, validate_chat_transcript,
};

use super::OPENAI_CODEC_ID;
use super::common::{codec_error, codec_eval_to_codec, map_field};

/// Decodes OpenAI request JSON into a validated chat-transcript expression.
pub fn decode_openai_request(input: Input) -> Result<Expr> {
    decode_openai_request_for_codec(OPENAI_CODEC_ID, input)
}

pub(in crate::providers) fn decode_openai_request_for_codec(
    codec: CodecId,
    input: Input,
) -> Result<Expr> {
    let source = domain_input_text(codec, input)?;
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    budget.check_input_bytes(codec, source.len())?;
    let value = serde_json::from_str::<Value>(&source).map_err(|err| codec_error(codec, err))?;
    let request = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "openai request must be a json object"))?;
    let model = string_member(codec, request, "model")?.to_owned();
    let (task, messages) = request_task_and_messages(codec, request)?;
    let mut entries = vec![
        (Expr::Symbol(Symbol::new("model-request")), Expr::Bool(true)),
        (Expr::Symbol(Symbol::new("task")), task),
        (Expr::Symbol(Symbol::new("messages")), Expr::List(messages)),
        (Expr::Symbol(Symbol::new("model")), Expr::String(model)),
    ];
    if let Some(stream) = request.get("stream").and_then(Value::as_bool) {
        entries.push((Expr::Symbol(Symbol::new("stream")), Expr::Bool(stream)));
    }
    if let Some(privacy) = request.get("privacy").and_then(Value::as_str) {
        entries.push((
            Expr::Symbol(Symbol::new("privacy")),
            Expr::String(privacy.to_owned()),
        ));
    }
    push_projected_field(
        codec,
        &mut budget,
        request,
        &mut entries,
        "budget",
        "budget",
    )?;
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

/// Decodes an OpenAI chat-completion response body into a model-response
/// transcript, optionally embedding the raw provider JSON.
pub fn decode_openai_response(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    budget.check_input_bytes(OPENAI_CODEC_ID, body.len())?;
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| Error::Eval(format!("openai codec returned invalid json: {err}")))?;
    response_expr_from_json(runner, model, &value, include_raw, &mut budget)
}

/// Decodes an OpenAI chat-completion SSE body into a single model-response
/// transcript, optionally embedding the raw provider chunks.
pub fn decode_openai_stream(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    budget.check_input_bytes(OPENAI_CODEC_ID, body.len())?;
    let text = std::str::from_utf8(body)
        .map_err(|err| Error::Eval(format!("openai stream is not valid utf-8: {err}")))?;
    let mut chunks = Vec::new();
    let mut combined = String::new();
    let mut usage_source = None;
    let mut stop_reason = Symbol::new("stop");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(payload)
            .map_err(|err| Error::Eval(format!("openai stream returned invalid json: {err}")))?;
        if let Some(error) = error_message(value.as_object()) {
            return error_response_expr(
                runner,
                model,
                error,
                include_raw,
                Some(&value),
                &mut budget,
            );
        }
        if usage_expr(
            value.as_object().ok_or_else(|| {
                Error::Eval("openai stream chunk must be a json object".to_owned())
            })?,
        )?
        .is_some()
        {
            usage_source = Some(value.clone());
        }
        if let Some(text) = stream_delta_text(&value)? {
            combined.push_str(text);
        }
        if let Some(reason) = stream_finish_reason(&value) {
            stop_reason = Symbol::new(reason);
        }
        chunks.push(value);
    }
    if chunks.is_empty() {
        return Err(Error::Eval(
            "openai stream did not contain any response chunks".to_owned(),
        ));
    }
    let mut entries =
        match model_response_expr(runner, model, vec![text_part(&combined)], stop_reason) {
            Expr::Map(entries) => entries,
            _ => unreachable!("model_response_expr always returns a map"),
        };
    if let Some(source) = usage_source.as_ref()
        && let Some(object) = source.as_object()
        && let Some(usage) = usage_expr(object)?
    {
        entries.push((Expr::Symbol(Symbol::new("usage")), usage));
    }
    if include_raw {
        let raw = chunks
            .iter()
            .map(|chunk| {
                project_json_to_expr_budgeted(
                    chunk,
                    JsonProjectionMode::UntaggedInterop,
                    OPENAI_CODEC_ID,
                    &mut budget,
                    0,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        entries.push((
            Expr::Symbol(Symbol::new("raw-provider-response")),
            Expr::List(raw),
        ));
    }
    Ok(Expr::Map(entries))
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

fn response_expr_from_json(
    runner: Symbol,
    model: &str,
    value: &Value,
    include_raw: bool,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let response = value
        .as_object()
        .ok_or_else(|| Error::Eval("openai response must be a json object".to_owned()))?;
    if let Some(error) = error_message(Some(response)) {
        return error_response_expr(runner, model, error, include_raw, Some(value), budget);
    }
    let choice = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Eval("openai response missing choices[0]".to_owned()))?;
    let message = choice
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Eval("openai response missing choices[0].message".to_owned()))?;
    let stop_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let mut entries = match model_response_expr(
        runner,
        model,
        message_content(message, budget)?,
        Symbol::new(stop_reason),
    ) {
        Expr::Map(entries) => entries,
        _ => unreachable!("model_response_expr always returns a map"),
    };
    if let Some(usage) = usage_expr(response)? {
        entries.push((Expr::Symbol(Symbol::new("usage")), usage));
    }
    if include_raw {
        entries.push((
            Expr::Symbol(Symbol::new("raw-provider-response")),
            project_json_to_expr_budgeted(
                value,
                JsonProjectionMode::UntaggedInterop,
                OPENAI_CODEC_ID,
                budget,
                0,
            )?,
        ));
    }
    Ok(Expr::Map(entries))
}

fn error_response_expr(
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
            project_json_to_expr_budgeted(
                raw,
                JsonProjectionMode::UntaggedInterop,
                OPENAI_CODEC_ID,
                budget,
                0,
            )?,
        ));
    }
    Ok(Expr::Map(entries))
}

fn error_message(object: Option<&Map<String, Value>>) -> Option<String> {
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

fn request_task_and_messages(
    codec: CodecId,
    request: &Map<String, Value>,
) -> Result<(Expr, Vec<Expr>)> {
    if let Some(messages) = request.get("messages").and_then(Value::as_array) {
        let (task_message, prior_messages) = messages
            .split_last()
            .ok_or_else(|| codec_error(codec, "openai request messages must not be empty"))?;
        return Ok((
            Expr::String(message_text(codec, task_message)?),
            prior_messages
                .iter()
                .map(|message| message_expr(codec, message))
                .collect::<Result<Vec<_>>>()?,
        ));
    }
    let input = request
        .get("input")
        .ok_or_else(|| codec_error(codec, "openai request missing input"))?;
    Ok((Expr::String(input_text_value(codec, input)?), Vec::new()))
}

fn input_text_value(codec: CodecId, value: &Value) -> Result<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        _ => Err(codec_error(codec, "openai request input must be a string")),
    }
}

fn message_expr(codec: CodecId, value: &Value) -> Result<Expr> {
    let object = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "openai message must be an object"))?;
    let role = string_member(codec, object, "role")?;
    Ok(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("role")),
            Expr::Symbol(Symbol::new(role)),
        ),
        (
            Expr::Symbol(Symbol::new("content")),
            Expr::List(content_parts(codec, object.get("content"))?),
        ),
    ]))
}

fn message_text(codec: CodecId, value: &Value) -> Result<String> {
    let object = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "openai task message must be an object"))?;
    let role = string_member(codec, object, "role")?;
    if role != "user" {
        return Err(codec_error(
            codec,
            "openai request final message must have role user",
        ));
    }
    let parts = content_parts(codec, object.get("content"))?;
    parts
        .iter()
        .map(text_from_part)
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join("\n"))
}

fn content_parts(codec: CodecId, content: Option<&Value>) -> Result<Vec<Expr>> {
    match content {
        Some(Value::String(text)) => Ok(vec![text_part(text)]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| request_part_from_json(codec, part))
            .collect(),
        Some(Value::Null) | None => Ok(Vec::new()),
        _ => Err(codec_error(
            codec,
            "openai message content must be string, array, or null",
        )),
    }
}

fn request_part_from_json(codec: CodecId, value: &Value) -> Result<Expr> {
    let object = value
        .as_object()
        .ok_or_else(|| codec_error(codec, "openai content part must be an object"))?;
    let kind = object.get("type").and_then(Value::as_str).unwrap_or("text");
    match kind {
        "text" => Ok(text_part(
            object
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| codec_error(codec, "openai text content part missing text"))?,
        )),
        other => Err(codec_error(
            codec,
            format!("openai content part type {other} is not supported"),
        )),
    }
}

fn message_content(message: &Map<String, Value>, budget: &mut DecodeBudget) -> Result<Vec<Expr>> {
    let mut parts = match message.get("content") {
        Some(Value::String(text)) => Ok(vec![text_part(text)]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(response_part_from_json)
            .collect::<Result<Vec<_>>>(),
        Some(Value::Null) | None => Ok(Vec::new()),
        _ => Err(Error::Eval(
            "openai response message content must be string, array, or null".to_owned(),
        )),
    }?;
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        parts.extend(
            tool_calls
                .iter()
                .map(|call| response_tool_call_part(call, budget))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Ok(parts)
}

fn response_part_from_json(value: &Value) -> Result<Expr> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Eval("openai response content part must be an object".to_owned()))?;
    let kind = object.get("type").and_then(Value::as_str).unwrap_or("text");
    match kind {
        "text" => Ok(text_part(
            object
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Eval("openai response text part missing text".to_owned()))?,
        )),
        other => Err(Error::Eval(format!(
            "openai response content part type {other} is not supported"
        ))),
    }
}

fn response_tool_call_part(value: &Value, budget: &mut DecodeBudget) -> Result<Expr> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Eval("openai tool call must be an object".to_owned()))?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Eval("openai tool call missing id".to_owned()))?;
    let function = object
        .get("function")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Eval("openai tool call missing function".to_owned()))?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Eval("openai tool call function missing name".to_owned()))?;
    Ok(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("type")),
            Expr::Symbol(Symbol::new("tool-call")),
        ),
        (Expr::Symbol(Symbol::new("id")), Expr::String(id.to_owned())),
        (
            Expr::Symbol(Symbol::new("name")),
            Expr::String(name.to_owned()),
        ),
        (
            Expr::Symbol(Symbol::new("arguments")),
            openai_tool_arguments_expr(function.get("arguments"), budget)?,
        ),
    ]))
}

fn openai_tool_arguments_expr(
    arguments: Option<&Value>,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let parsed;
    let empty = Value::Object(Map::new());
    let value = match arguments {
        Some(Value::String(text)) if !text.trim().is_empty() => {
            parsed = serde_json::from_str::<Value>(text).map_err(|err| {
                Error::Eval(format!("openai tool call arguments must be json: {err}"))
            })?;
            &parsed
        }
        Some(Value::String(_)) | Some(Value::Null) | None => &empty,
        Some(value) => value,
    };
    project_json_to_expr_budgeted(
        value,
        JsonProjectionMode::UntaggedInterop,
        OPENAI_CODEC_ID,
        budget,
        0,
    )
}

fn usage_expr(response: &Map<String, Value>) -> Result<Option<Expr>> {
    let Some(usage) = response.get("usage").and_then(Value::as_object) else {
        return Ok(None);
    };
    let input = usage.get("prompt_tokens").and_then(json_number_to_u64);
    let output = usage.get("completion_tokens").and_then(json_number_to_u64);
    let total = usage.get("total_tokens").and_then(json_number_to_u64);
    Ok(Some(Expr::Map(usage_record(input, output, total))))
}

fn stream_delta_text(value: &Value) -> Result<Option<&str>> {
    let choice = stream_choice(value)?;
    Ok(choice
        .and_then(|choice| choice.get("delta"))
        .and_then(Value::as_object)
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str))
}

fn stream_finish_reason(value: &Value) -> Option<&str> {
    stream_choice(value)
        .ok()
        .flatten()
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
}

fn stream_choice(value: &Value) -> Result<Option<&Map<String, Value>>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Eval("openai stream chunk must be a json object".to_owned()))?;
    Ok(object
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(Value::as_object))
}

fn text_from_part(part: &Expr) -> Result<String> {
    let Expr::Map(entries) = part else {
        return Err(Error::Eval(
            "openai content part transcript must be a map".to_owned(),
        ));
    };
    match map_field(entries, "text")? {
        Expr::String(text) => Ok(text.clone()),
        _ => Err(Error::Eval(
            "openai text content part text field must be a string".to_owned(),
        )),
    }
}

fn string_member<'a>(
    codec: CodecId,
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| codec_error(codec, format!("openai request missing string {name}")))
}
