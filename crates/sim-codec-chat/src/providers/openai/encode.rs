use serde_json::{Value, json};
use sim_kernel::{CodecId, Error, Expr, Result};

use crate::output_grammar::{OutputGrammarDialect, output_grammar_required, output_grammar_text};
use crate::{is_model_request_expr, validate_chat_transcript};

use super::OpenAiRequestOptions;
use super::common::{
    codec_error, codec_eval_to_codec, flatten_expr, list_field, map_field, marker_is_true,
    optional_u64_field, string_field, symbol_field,
};

/// Encodes a model-request transcript into OpenAI chat-completion JSON.
pub fn encode_openai_request(expr: &Expr, options: &OpenAiRequestOptions) -> Result<Vec<u8>> {
    if !is_model_request_expr(expr) {
        return Err(Error::Eval(
            "openai codec expects a model-request transcript".to_owned(),
        ));
    }
    validate_chat_transcript(expr)?;
    let entries = request_entries(expr)?;
    let mut payload = json!({
        "model": options.model,
        "stream": options.stream,
        "messages": transcript_messages(expr)?,
        "tools": if options.tools { Value::Array(Vec::new()) } else { Value::Null },
    });
    attach_output_grammar(entries, &mut payload)?;
    if options.stream
        && let Some(object) = payload.as_object_mut()
    {
        object.insert("stream_options".to_owned(), json!({"include_usage": true}));
    }
    serde_json::to_vec(&payload)
        .map_err(|err| Error::Eval(format!("openai codec failed to encode request: {err}")))
}

fn attach_output_grammar(entries: &[(Expr, Expr)], payload: &mut Value) -> Result<()> {
    let Some(grammar) = output_grammar_text(entries, OutputGrammarDialect::JsonSchema)? else {
        return Ok(());
    };
    let schema = serde_json::from_str::<Value>(&grammar)
        .map_err(|err| Error::Eval(format!("openai output grammar is not json schema: {err}")))?;
    let Some(object) = payload.as_object_mut() else {
        return Err(Error::Eval(
            "openai request payload must be a json object".to_owned(),
        ));
    };
    object.insert(
        "response_format".to_owned(),
        json!({
            "type": "json_schema",
            "json_schema": {
                "name": "sim_output",
                "strict": output_grammar_required(entries)?,
                "schema": schema,
            }
        }),
    );
    Ok(())
}

fn request_entries(expr: &Expr) -> Result<&[(Expr, Expr)]> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "openai codec expects request transcript as a map".to_owned(),
        ));
    };
    Ok(entries)
}

/// Encodes a model-response transcript into OpenAI chat-completion JSON.
pub fn encode_openai_response(expr: &Expr) -> Result<Vec<u8>> {
    let value = response_json(expr)?;
    serde_json::to_vec(&value)
        .map_err(|err| Error::Eval(format!("openai codec failed to encode response: {err}")))
}

pub(in crate::providers) fn encode_openai_response_for_codec(
    codec: CodecId,
    expr: &Expr,
) -> Result<String> {
    if !marker_is_true(expr, "model-response") {
        return Err(codec_error(
            codec,
            "openai codec expects a model-response transcript",
        ));
    }
    validate_chat_transcript(expr).map_err(|err| codec_eval_to_codec(codec, err))?;
    let value = response_json(expr).map_err(|err| codec_eval_to_codec(codec, err))?;
    serde_json::to_string(&value).map_err(|err| codec_error(codec, err))
}

fn transcript_messages(expr: &Expr) -> Result<Vec<Value>> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "openai codec expects request transcript as a map".to_owned(),
        ));
    };
    let mut messages = list_field(map_field(entries, "messages")?)?
        .iter()
        .map(message_to_json)
        .collect::<Result<Vec<_>>>()?;
    messages.push(json!({
        "role": "user",
        "content": [{
            "type": "text",
            "text": flatten_expr(map_field(entries, "task")?),
        }],
    }));
    Ok(messages)
}

fn message_to_json(expr: &Expr) -> Result<Value> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval("openai codec message must be a map".to_owned()));
    };
    Ok(json!({
        "role": symbol_field(entries, "role")?,
        "content": list_field(map_field(entries, "content")?)?
            .iter()
            .map(content_part_to_json)
            .collect::<Result<Vec<_>>>()?,
    }))
}

fn content_part_to_json(expr: &Expr) -> Result<Value> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "openai codec content part must be a map".to_owned(),
        ));
    };
    match symbol_field(entries, "type")?.as_str() {
        "text" => Ok(json!({
            "type": "text",
            "text": string_field(entries, "text")?,
        })),
        other => Err(Error::Eval(format!(
            "openai codec does not support content part type {other}"
        ))),
    }
}

fn response_json(expr: &Expr) -> Result<Value> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "openai codec expects response transcript as a map".to_owned(),
        ));
    };
    let model = string_field(entries, "model")?;
    let finish_reason = symbol_field(entries, "stop-reason")?;
    Ok(json!({
        "id": "chatcmpl-sim",
        "object": "chat.completion",
        "created": 0,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": response_text(entries)?,
            },
            "finish_reason": finish_reason,
        }],
        "usage": response_usage(entries)?,
    }))
}

fn response_text(entries: &[(Expr, Expr)]) -> Result<String> {
    list_field(map_field(entries, "content")?)?
        .iter()
        .map(text_content)
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join(""))
}

fn text_content(expr: &Expr) -> Result<String> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "openai codec content part must be a map".to_owned(),
        ));
    };
    match symbol_field(entries, "type")?.as_str() {
        "text" => string_field(entries, "text"),
        other => Err(Error::Eval(format!(
            "openai codec does not support content part type {other}"
        ))),
    }
}

fn response_usage(entries: &[(Expr, Expr)]) -> Result<Value> {
    let Some(usage) = entries.iter().find_map(|(field, value)| match field {
        Expr::Symbol(symbol) if symbol.name.as_ref() == "usage" => Some(value),
        _ => None,
    }) else {
        return Ok(Value::Null);
    };
    let Expr::Map(fields) = usage else {
        return Err(Error::Eval(
            "openai codec usage field must be a map".to_owned(),
        ));
    };
    let prompt = optional_u64_field(fields, "input-tokens")?;
    let completion = optional_u64_field(fields, "output-tokens")?;
    let total = optional_u64_field(fields, "total-tokens")?.or_else(|| {
        prompt
            .zip(completion)
            .map(|(left, right)| left.saturating_add(right))
    });
    Ok(json!({
        "prompt_tokens": prompt.unwrap_or(0),
        "completion_tokens": completion.unwrap_or(0),
        "total_tokens": total.unwrap_or(0),
    }))
}
