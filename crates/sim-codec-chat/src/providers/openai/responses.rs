//! OpenAI Responses API projection on the canonical chat model contract.

use serde_json::{Map, Value, json};
use sim_codec::{DecodeBudget, DecodeLimits, Input, domain_input_text};
use sim_codec_json::{JsonProjectionMode, json_number_to_u64, project_json_to_expr_budgeted};
use sim_kernel::{Error, Expr, Result, Symbol};

use crate::output_grammar::{OutputGrammarDialect, output_grammar_required, output_grammar_text};
use crate::{
    is_model_request_expr, model_response_expr, text_part, usage_record, validate_chat_transcript,
};

use super::super::model_params::{attach_bridge_model_params, model_param_value};
use super::common::{codec_error, flatten_expr, list_field, map_field, string_field, symbol_field};
use super::{OPENAI_CODEC_ID, OpenAiRequestOptions};

const RESERVED_FIELDS: &[&str] = &["model", "stream", "input", "tools", "text"];

/// Encodes a canonical model request as an OpenAI Responses request body.
pub fn encode_openai_responses_request(
    expr: &Expr,
    options: &OpenAiRequestOptions,
) -> Result<Vec<u8>> {
    if !is_model_request_expr(expr) {
        return Err(Error::Eval(
            "openai Responses codec expects a model-request transcript".into(),
        ));
    }
    validate_chat_transcript(expr)?;
    let Expr::Map(entries) = expr else {
        unreachable!()
    };
    let mut object = Map::new();
    object.insert("model".into(), Value::String(options.model.clone()));
    object.insert("stream".into(), Value::Bool(options.stream));
    object.insert("input".into(), Value::Array(responses_input(entries)?));
    if options.tools || optional_field(entries, "tools").is_some() {
        object.insert("tools".into(), request_tools(entries)?);
    }
    attach_text_format(entries, &mut object)?;
    attach_bridge_model_params(entries, &mut object, RESERVED_FIELDS, "openai Responses")?;
    serde_json::to_vec(&Value::Object(object)).map_err(|err| {
        Error::Eval(format!(
            "openai Responses codec failed to encode request: {err}"
        ))
    })
}

fn responses_input(entries: &[(Expr, Expr)]) -> Result<Vec<Value>> {
    let mut input = list_field(map_field(entries, "messages")?)?
        .iter()
        .map(message_to_input)
        .collect::<Result<Vec<_>>>()?;
    input.push(json!({"type":"message", "role":"user", "content":[{
        "type":"input_text", "text": flatten_expr(map_field(entries, "task")?)
    }]}));
    Ok(input)
}

fn message_to_input(expr: &Expr) -> Result<Value> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval("openai Responses message must be a map".into()));
    };
    Ok(
        json!({"type":"message", "role":symbol_field(entries, "role")?, "content":
        list_field(map_field(entries, "content")?)?.iter().map(input_part).collect::<Result<Vec<_>>>()?}),
    )
}

fn input_part(expr: &Expr) -> Result<Value> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "openai Responses content part must be a map".into(),
        ));
    };
    match symbol_field(entries, "type")?.as_str() {
        "text" => Ok(json!({"type":"input_text", "text":string_field(entries, "text")?})),
        kind => Err(Error::Eval(format!(
            "openai Responses does not support input content part {kind}"
        ))),
    }
}

fn request_tools(entries: &[(Expr, Expr)]) -> Result<Value> {
    let Some(tools) = optional_field(entries, "tools") else {
        return Ok(Value::Array(Vec::new()));
    };
    let Value::Array(tools) = model_param_value(tools) else {
        return Err(Error::Eval(
            "openai Responses tools must be a list".to_owned(),
        ));
    };
    tools
        .into_iter()
        .map(|tool| {
            let mut tool = tool
                .as_object()
                .cloned()
                .ok_or_else(|| Error::Eval("openai Responses tool must be a map".to_owned()))?;
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err(Error::Eval(
                    "openai Responses tool type must be function".to_owned(),
                ));
            }
            if let Some(Value::Object(function)) = tool.remove("function") {
                for field in ["name", "description", "parameters", "strict"] {
                    if let Some(value) = function.get(field) {
                        tool.insert(field.to_owned(), value.clone());
                    }
                }
            }
            if !matches!(tool.get("name"), Some(Value::String(_)))
                || !matches!(tool.get("parameters"), Some(Value::Object(_)))
            {
                return Err(Error::Eval(
                    "openai Responses function tool requires name and parameters".to_owned(),
                ));
            }
            Ok(Value::Object(tool))
        })
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

fn attach_text_format(entries: &[(Expr, Expr)], object: &mut Map<String, Value>) -> Result<()> {
    let Some(grammar) = output_grammar_text(entries, OutputGrammarDialect::JsonSchema)? else {
        return Ok(());
    };
    let schema: Value = serde_json::from_str(&grammar).map_err(|err| {
        Error::Eval(format!(
            "openai Responses output grammar is not json schema: {err}"
        ))
    })?;
    object.insert(
        "text".into(),
        json!({"format":{"type":"json_schema", "name":"sim_output",
        "strict":output_grammar_required(entries)?, "schema":schema}}),
    );
    Ok(())
}

/// Decodes an OpenAI Responses request into the canonical model request.
pub fn decode_openai_responses_request(input: Input) -> Result<Expr> {
    decode_openai_responses_request_with_limits(input, DecodeLimits::default())
}

/// Decodes an OpenAI Responses request under explicit limits.
pub fn decode_openai_responses_request_with_limits(
    input: Input,
    limits: DecodeLimits,
) -> Result<Expr> {
    let source = domain_input_text(OPENAI_CODEC_ID, input)?;
    let mut budget = DecodeBudget::new(limits);
    budget.check_input_bytes(OPENAI_CODEC_ID, source.len())?;
    let value: Value =
        serde_json::from_str(&source).map_err(|err| codec_error(OPENAI_CODEC_ID, err))?;
    let request = object(&value, "openai Responses request")?;
    let model = required_str(request, "model", "openai Responses request")?;
    let input = request
        .get("input")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            codec_error(
                OPENAI_CODEC_ID,
                "openai Responses request missing input array",
            )
        })?;
    budget.check_collection_len(OPENAI_CODEC_ID, input.len())?;
    let (last, prior) = input
        .split_last()
        .ok_or_else(|| codec_error(OPENAI_CODEC_ID, "openai Responses input must not be empty"))?;
    let task = decode_input_message(last, Some("user"), &mut budget)?.1;
    let messages = prior
        .iter()
        .map(|item| {
            decode_input_message(item, None, &mut budget).map(|(role, text)| {
                Expr::Map(vec![
                    key("role", Expr::Symbol(Symbol::new(role))),
                    key("content", Expr::List(vec![text_part(&text)])),
                ])
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut entries = vec![
        key("model-request", Expr::Bool(true)),
        key("task", Expr::String(task)),
        key("messages", Expr::List(messages)),
        key("model", Expr::String(model.into())),
    ];
    if let Some(stream) = request.get("stream").and_then(Value::as_bool) {
        entries.push(key("stream", Expr::Bool(stream)));
    }
    for (wire, canonical) in [("tools", "tools"), ("tool_choice", "tool-choice")] {
        if let Some(value) = request.get(wire) {
            entries.push(key(canonical, project(value, &mut budget)?));
        }
    }
    let expr = Expr::Map(entries);
    validate_chat_transcript(&expr)?;
    Ok(expr)
}

fn decode_input_message(
    value: &Value,
    required_role: Option<&str>,
    budget: &mut DecodeBudget,
) -> Result<(String, String)> {
    let item = object(value, "openai Responses input item")?;
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return Err(codec_error(
            OPENAI_CODEC_ID,
            "openai Responses input item must have type message",
        ));
    }
    let role = required_str(item, "role", "openai Responses input message")?;
    if required_role.is_some_and(|expected| expected != role) {
        return Err(codec_error(
            OPENAI_CODEC_ID,
            "openai Responses final input message must have role user",
        ));
    }
    let parts = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            codec_error(
                OPENAI_CODEC_ID,
                "openai Responses input message missing content array",
            )
        })?;
    budget.check_collection_len(OPENAI_CODEC_ID, parts.len())?;
    let mut text = Vec::new();
    for part in parts {
        let part = object(part, "openai Responses input content")?;
        if part.get("type").and_then(Value::as_str) != Some("input_text") {
            return Err(codec_error(
                OPENAI_CODEC_ID,
                "openai Responses input content must have type input_text",
            ));
        }
        let value = required_str(part, "text", "openai Responses input content")?;
        budget.check_string_bytes(OPENAI_CODEC_ID, value.len())?;
        text.push(value);
    }
    Ok((role.into(), text.join("\n")))
}

/// Decodes one non-streaming OpenAI Responses body.
pub fn decode_openai_responses_response(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    decode_openai_responses_response_with_limits(
        runner,
        model,
        body,
        include_raw,
        DecodeLimits::default(),
    )
}

/// Decodes one non-streaming OpenAI Responses body under explicit limits.
pub fn decode_openai_responses_response_with_limits(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
    limits: DecodeLimits,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(limits);
    budget.check_input_bytes(OPENAI_CODEC_ID, body.len())?;
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| Error::Eval(format!("openai Responses returned invalid json: {err}")))?;
    response_from_value(runner, model, &value, include_raw, &mut budget)
}

/// Decodes a complete OpenAI Responses SSE transcript.
pub fn decode_openai_responses_stream(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    decode_openai_responses_stream_with_limits(
        runner,
        model,
        body,
        include_raw,
        DecodeLimits::default(),
    )
}

/// Decodes a complete OpenAI Responses SSE transcript under explicit limits.
pub fn decode_openai_responses_stream_with_limits(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
    limits: DecodeLimits,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(limits);
    budget.check_input_bytes(OPENAI_CODEC_ID, body.len())?;
    let text = std::str::from_utf8(body)
        .map_err(|err| Error::Eval(format!("openai Responses stream is not utf-8: {err}")))?;
    let mut frames = Vec::new();
    let mut terminal = None;
    let mut aggregate = 0usize;
    for block in text.split("\n\n") {
        let mut event_name = None;
        let mut data = None;
        for line in block
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with(':'))
        {
            if let Some(name) = line.strip_prefix("event:") {
                event_name = Some(name.trim());
            } else if let Some(payload) = line.strip_prefix("data:") {
                if data.is_some() {
                    return Err(codec_error(
                        OPENAI_CODEC_ID,
                        "openai Responses SSE frame has multiple data fields",
                    ));
                }
                data = Some(payload.trim());
            } else {
                return Err(codec_error(
                    OPENAI_CODEC_ID,
                    "openai Responses SSE frame has unknown required structure",
                ));
            }
        }
        let Some(payload) = data else { continue };
        aggregate = aggregate.checked_add(payload.len()).ok_or_else(|| {
            codec_error(
                OPENAI_CODEC_ID,
                "openai Responses stream byte count overflow",
            )
        })?;
        budget.check_input_bytes(OPENAI_CODEC_ID, aggregate)?;
        let value: Value = serde_json::from_str(payload).map_err(|err| {
            Error::Eval(format!(
                "openai Responses stream returned invalid json: {err}"
            ))
        })?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .or(event_name)
            .ok_or_else(|| {
                codec_error(OPENAI_CODEC_ID, "openai Responses SSE event missing type")
            })?;
        if event_name.is_some_and(|name| name != kind) {
            return Err(codec_error(
                OPENAI_CODEC_ID,
                "openai Responses SSE event/type mismatch",
            ));
        }
        match kind {
            "response.created"
            | "response.in_progress"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.output_text.delta"
            | "response.refusal.delta"
            | "response.function_call_arguments.delta"
            | "response.function_call_arguments.done"
            | "response.output_text.done"
            | "response.content_part.done"
            | "response.output_item.done" => {}
            "response.completed" | "response.incomplete" | "response.failed" => {
                if terminal.is_some() {
                    return Err(codec_error(
                        OPENAI_CODEC_ID,
                        "openai Responses stream has multiple terminal events",
                    ));
                }
                terminal = value.get("response").cloned();
                if terminal.is_none() {
                    return Err(codec_error(
                        OPENAI_CODEC_ID,
                        "openai Responses terminal event missing response",
                    ));
                }
            }
            "error" => {
                return Err(codec_error(
                    OPENAI_CODEC_ID,
                    value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("openai Responses stream error"),
                ));
            }
            _ => {
                return Err(codec_error(
                    OPENAI_CODEC_ID,
                    format!("openai Responses unknown event type {kind}"),
                ));
            }
        }
        budget.check_collection_len(OPENAI_CODEC_ID, frames.len() + 1)?;
        frames.push(value);
    }
    let terminal = terminal.ok_or_else(|| {
        codec_error(
            OPENAI_CODEC_ID,
            "openai Responses stream ended without terminal event",
        )
    })?;
    let mut response = response_from_value(runner, model, &terminal, false, &mut budget)?;
    if include_raw && let Expr::Map(entries) = &mut response {
        entries.push(key(
            "raw-provider-response",
            Expr::List(
                frames
                    .iter()
                    .map(|v| project(v, &mut budget))
                    .collect::<Result<_>>()?,
            ),
        ));
    }
    Ok(response)
}

fn response_from_value(
    runner: Symbol,
    fallback_model: &str,
    value: &Value,
    include_raw: bool,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let response = object(value, "openai Responses response")?;
    let status = required_str(response, "status", "openai Responses response")?;
    if !matches!(status, "completed" | "incomplete" | "failed") {
        return Err(codec_error(
            OPENAI_CODEC_ID,
            format!("openai Responses response has unknown terminal status {status}"),
        ));
    }
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            codec_error(
                OPENAI_CODEC_ID,
                "openai Responses response missing output array",
            )
        })?;
    budget.check_collection_len(OPENAI_CODEC_ID, output.len())?;
    let mut content = Vec::new();
    for item in output {
        decode_output_item(item, &mut content, budget)?;
    }
    let stop = match status {
        "completed" => "stop",
        "incomplete" => "incomplete",
        _ => "error",
    };
    let model = response
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model);
    let mut entries = match model_response_expr(runner, model, content, Symbol::new(stop)) {
        Expr::Map(v) => v,
        _ => unreachable!(),
    };
    if let Some(id) = response.get("id").and_then(Value::as_str) {
        entries.push(key("provider-request-id", Expr::String(id.into())));
    }
    if let Some(usage) = response.get("usage").and_then(Value::as_object) {
        entries.push(key(
            "usage",
            Expr::Map(usage_record(
                usage.get("input_tokens").and_then(json_number_to_u64),
                usage.get("output_tokens").and_then(json_number_to_u64),
                usage.get("total_tokens").and_then(json_number_to_u64),
            )),
        ));
    }
    if include_raw {
        entries.push(key("raw-provider-response", project(value, budget)?));
    }
    let expr = Expr::Map(entries);
    validate_chat_transcript(&expr)?;
    Ok(expr)
}

fn decode_output_item(
    value: &Value,
    content: &mut Vec<Expr>,
    budget: &mut DecodeBudget,
) -> Result<()> {
    let item = object(value, "openai Responses output item")?;
    match required_str(item, "type", "openai Responses output item")? {
        "message" => {
            let parts = item
                .get("content")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    codec_error(OPENAI_CODEC_ID, "openai Responses message missing content")
                })?;
            budget.check_collection_len(OPENAI_CODEC_ID, parts.len())?;
            for part in parts {
                let part = object(part, "openai Responses output content")?;
                match required_str(part, "type", "openai Responses output content")? {
                    "output_text" => {
                        let text = required_str(part, "text", "openai Responses output_text")?;
                        budget.check_string_bytes(OPENAI_CODEC_ID, text.len())?;
                        content.push(text_part(text));
                    }
                    "refusal" => {
                        let text = required_str(part, "refusal", "openai Responses refusal")?;
                        budget.check_string_bytes(OPENAI_CODEC_ID, text.len())?;
                        content.push(Expr::Map(vec![
                            key("type", Expr::Symbol(Symbol::new("refusal"))),
                            key("text", Expr::String(text.into())),
                        ]));
                    }
                    kind => {
                        return Err(codec_error(
                            OPENAI_CODEC_ID,
                            format!("openai Responses unknown output content type {kind}"),
                        ));
                    }
                }
            }
        }
        "function_call" => {
            let arguments = required_str(item, "arguments", "openai Responses function call")?;
            budget.check_string_bytes(OPENAI_CODEC_ID, arguments.len())?;
            let args: Value = serde_json::from_str(arguments).map_err(|err| {
                codec_error(
                    OPENAI_CODEC_ID,
                    format!("openai Responses function arguments must be json: {err}"),
                )
            })?;
            content.push(Expr::Map(vec![
                key("type", Expr::Symbol(Symbol::new("tool-call"))),
                key(
                    "id",
                    Expr::String(
                        required_str(item, "call_id", "openai Responses function call")?.into(),
                    ),
                ),
                key(
                    "name",
                    Expr::String(
                        required_str(item, "name", "openai Responses function call")?.into(),
                    ),
                ),
                key("arguments", project(&args, budget)?),
            ]));
        }
        // Reasoning output may carry encrypted or provider-internal material.
        // Its presence is structurally recognized but it is never projected
        // onto the canonical response or exposed through content.
        "reasoning" => {
            required_str(item, "id", "openai Responses reasoning item")?;
        }
        kind => {
            return Err(codec_error(
                OPENAI_CODEC_ID,
                format!("openai Responses unknown output item type {kind}"),
            ));
        }
    }
    budget.check_collection_len(OPENAI_CODEC_ID, content.len())
}

fn optional_field<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    entries.iter().find_map(|(key, value)| match key {
        Expr::Symbol(s) | Expr::Local(s) if s.name.as_ref() == name => Some(value),
        Expr::String(s) if s == name => Some(value),
        _ => None,
    })
}
fn object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| codec_error(OPENAI_CODEC_ID, format!("{context} must be an object")))
}
fn required_str<'a>(object: &'a Map<String, Value>, field: &str, context: &str) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| codec_error(OPENAI_CODEC_ID, format!("{context} missing string {field}")))
}
fn key(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}
fn project(value: &Value, budget: &mut DecodeBudget) -> Result<Expr> {
    project_json_to_expr_budgeted(
        value,
        JsonProjectionMode::UntaggedInterop,
        OPENAI_CODEC_ID,
        budget,
        0,
    )
}
