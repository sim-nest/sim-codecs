use serde_json::{Map, Value, json};
use sim_kernel::{CodecId, Error, Expr, Result};

use crate::output_grammar::reject_output_grammar;
use crate::{is_model_request_expr, validate_chat_transcript};

use super::AnthropicRequestOptions;
use super::common::{
    codec_error, codec_eval_to_codec, expr_entries, flatten_expr, optional_expr, optional_string,
    required_list, required_string, required_symbol, sim_expr_to_json,
};
use crate::providers::model_params::attach_bridge_model_params;

const RESERVED_MODEL_PARAM_FIELDS: &[&str] = &[
    "model",
    "max_tokens",
    "stream",
    "system",
    "messages",
    "tools",
    "tool_choice",
];

/// Encodes a model-request transcript into Anthropic Messages JSON.
pub fn encode_anthropic_request(expr: &Expr, options: &AnthropicRequestOptions) -> Result<Vec<u8>> {
    if !is_model_request_expr(expr) {
        return Err(Error::Eval(
            "anthropic codec expects a model-request transcript".to_owned(),
        ));
    }
    validate_chat_transcript(expr)?;
    let entries = expr_entries(expr, "anthropic request transcript")?;
    reject_output_grammar(entries, "anthropic")?;
    let (messages, system) = transcript_messages(entries)?;
    let mut payload = Map::new();
    payload.insert("model".to_owned(), Value::String(options.model.clone()));
    payload.insert("max_tokens".to_owned(), json!(options.max_tokens));
    payload.insert("stream".to_owned(), Value::Bool(options.stream));
    if let Some(system) = system {
        payload.insert("system".to_owned(), Value::String(system));
    }
    payload.insert("messages".to_owned(), Value::Array(messages));
    if options.tools {
        payload.insert("tools".to_owned(), Value::Array(tool_schemas(entries)?));
    }
    if let Some(tool_choice) = optional_expr(entries, "tool-choice") {
        payload.insert("tool_choice".to_owned(), sim_expr_to_json(tool_choice));
    }
    attach_bridge_model_params(
        entries,
        &mut payload,
        RESERVED_MODEL_PARAM_FIELDS,
        "anthropic",
    )?;
    serde_json::to_vec(&Value::Object(payload))
        .map_err(|err| Error::Eval(format!("anthropic codec failed to encode request: {err}")))
}

/// Encodes a model-response transcript into Anthropic Messages JSON.
pub fn encode_anthropic_response(expr: &Expr) -> Result<Vec<u8>> {
    let value = response_json(expr)?;
    serde_json::to_vec(&value)
        .map_err(|err| Error::Eval(format!("anthropic codec failed to encode response: {err}")))
}

pub(super) fn encode_anthropic_response_for_codec(codec: CodecId, expr: &Expr) -> Result<String> {
    validate_chat_transcript(expr).map_err(|err| codec_eval_to_codec(codec, err))?;
    let value = response_json(expr).map_err(|err| codec_eval_to_codec(codec, err))?;
    serde_json::to_string(&value).map_err(|err| codec_error(codec, err))
}

fn transcript_messages(entries: &[(Expr, Expr)]) -> Result<(Vec<Value>, Option<String>)> {
    let mut messages = Vec::new();
    let mut system = Vec::new();
    for message in required_list(entries, "messages")? {
        let Some(rendered) = message_to_json(message, &mut system)? else {
            continue;
        };
        messages.push(rendered);
    }
    messages.push(json!({
        "role": "user",
        "content": [{
            "type": "text",
            "text": flatten_expr(
                optional_expr(entries, "task")
                    .ok_or_else(|| Error::Eval("anthropic request missing task field".to_owned()))?
            ),
        }],
    }));
    let system = (!system.is_empty()).then(|| system.join("\n\n"));
    Ok((messages, system))
}

fn message_to_json(expr: &Expr, system: &mut Vec<String>) -> Result<Option<Value>> {
    let entries = expr_entries(expr, "anthropic message")?;
    let role = required_symbol(entries, "role")?;
    if role.name.as_ref() == "system" && role.namespace.is_none() {
        system.push(system_message_text(entries)?);
        return Ok(None);
    }
    Ok(Some(json!({
        "role": role.name.as_ref(),
        "content": required_list(entries, "content")?
            .iter()
            .map(content_part_to_json)
            .collect::<Result<Vec<_>>>()?,
    })))
}

fn system_message_text(entries: &[(Expr, Expr)]) -> Result<String> {
    required_list(entries, "content")?
        .iter()
        .map(content_part_to_text)
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("\n"))
}

fn content_part_to_text(expr: &Expr) -> Result<String> {
    let entries = expr_entries(expr, "anthropic content part")?;
    match required_symbol(entries, "type")?.name.as_ref() {
        "text" => Ok(required_string(entries, "text")?.to_owned()),
        other => Err(Error::Eval(format!(
            "anthropic system message does not support content part type {other}"
        ))),
    }
}

fn content_part_to_json(expr: &Expr) -> Result<Value> {
    let entries = expr_entries(expr, "anthropic content part")?;
    match required_symbol(entries, "type")?.name.as_ref() {
        "text" => Ok(json!({
            "type": "text",
            "text": required_string(entries, "text")?,
        })),
        "tool-call" => Ok(json!({
            "type": "tool_use",
            "id": optional_string(entries, "id")?.unwrap_or_else(|| "toolu_sim".to_owned()),
            "name": required_string(entries, "name")?,
            "input": optional_expr(entries, "arguments")
                .or_else(|| optional_expr(entries, "input"))
                .map(sim_expr_to_json)
                .unwrap_or_else(|| Value::Object(Map::new())),
        })),
        "tool-result" => {
            let mut object = Map::new();
            object.insert("type".to_owned(), Value::String("tool_result".to_owned()));
            object.insert(
                "tool_use_id".to_owned(),
                Value::String(
                    optional_string_any(entries, &["tool-call-id", "id"])?
                        .unwrap_or_else(|| "toolu_sim".to_owned()),
                ),
            );
            object.insert(
                "content".to_owned(),
                Value::String(
                    optional_expr(entries, "output")
                        .map(flatten_expr)
                        .unwrap_or_else(|| "".to_owned()),
                ),
            );
            if let Some(Expr::Symbol(status)) = optional_expr(entries, "status")
                && status.name.as_ref() != "ok"
            {
                object.insert("is_error".to_owned(), Value::Bool(true));
            }
            Ok(Value::Object(object))
        }
        other => Err(Error::Eval(format!(
            "anthropic codec does not support content part type {other}"
        ))),
    }
}

fn tool_schemas(entries: &[(Expr, Expr)]) -> Result<Vec<Value>> {
    let Some(tools) = optional_expr(entries, "tools") else {
        return Ok(Vec::new());
    };
    let Expr::List(tools) = tools else {
        return Err(Error::Eval(
            "anthropic request tools field must be a list".to_owned(),
        ));
    };
    tools.iter().map(tool_schema).collect()
}

fn tool_schema(expr: &Expr) -> Result<Value> {
    let entries = expr_entries(expr, "anthropic tool schema")?;
    let name = optional_string_any(entries, &["name", "openai-name", "symbol"])?
        .ok_or_else(|| Error::Eval("anthropic tool schema missing name".to_owned()))?;
    let description = optional_string(entries, "description")?;
    let input_schema = optional_expr(entries, "input-schema")
        .or_else(|| optional_expr(entries, "input_schema"))
        .or_else(|| optional_expr(entries, "parameters"))
        .map(sim_expr_to_json)
        .unwrap_or_else(|| json!({"type":"object","properties":{}}));
    let mut object = Map::new();
    object.insert("name".to_owned(), Value::String(name));
    if let Some(description) = description {
        object.insert("description".to_owned(), Value::String(description));
    }
    object.insert("input_schema".to_owned(), input_schema);
    Ok(Value::Object(object))
}

fn optional_string_any(entries: &[(Expr, Expr)], keys: &[&str]) -> Result<Option<String>> {
    for key in keys {
        if let Some(value) = optional_string(entries, key)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn response_json(expr: &Expr) -> Result<Value> {
    let entries = expr_entries(expr, "anthropic response transcript")?;
    let model = required_string(entries, "model")?;
    let stop_reason = required_symbol(entries, "stop-reason")?;
    Ok(json!({
        "id": "msg_sim",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": required_list(entries, "content")?
            .iter()
            .map(content_part_to_json)
            .collect::<Result<Vec<_>>>()?,
        "stop_reason": stop_reason.name.as_ref().replace('-', "_"),
        "stop_sequence": Value::Null,
        "usage": response_usage(entries)?,
    }))
}

fn response_usage(entries: &[(Expr, Expr)]) -> Result<Value> {
    let Some(usage) = optional_expr(entries, "usage") else {
        return Ok(json!({"input_tokens":0,"output_tokens":0}));
    };
    let fields = expr_entries(usage, "anthropic usage field")?;
    Ok(json!({
        "input_tokens": optional_u64_field(fields, "input-tokens")?.unwrap_or(0),
        "output_tokens": optional_u64_field(fields, "output-tokens")?.unwrap_or(0),
    }))
}

fn optional_u64_field(entries: &[(Expr, Expr)], key: &str) -> Result<Option<u64>> {
    let Some(value) = optional_expr(entries, key) else {
        return Ok(None);
    };
    match value {
        Expr::Number(number) => number
            .canonical
            .parse::<u64>()
            .map(Some)
            .map_err(|err| Error::Eval(format!("anthropic usage field {key} invalid: {err}"))),
        Expr::String(text) => text
            .parse::<u64>()
            .map(Some)
            .map_err(|err| Error::Eval(format!("anthropic usage field {key} invalid: {err}"))),
        other => Err(Error::Eval(format!(
            "anthropic usage field {key} must be a number, found {other:?}"
        ))),
    }
}
