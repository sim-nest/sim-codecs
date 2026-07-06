//! Ollama provider bridge: encode a chat model-request transcript into an
//! Ollama JSON request, and decode Ollama JSON responses and streamed chunks
//! back into canonical chat transcript `Expr` values.

use serde_json::{Map, Value, json};
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_codec_json::{JsonProjectionMode, json_number_to_u64, project_json_to_expr_budgeted};
use sim_kernel::{CodecId, Error, Expr, Result, Symbol};
use sim_value::access;

use crate::{
    is_model_request_expr, model_response_expr, text_part, usage_record, validate_chat_transcript,
};

/// Codec id used to tag decode-budget errors raised while projecting an Ollama
/// provider response. The Ollama bridge is a set of free functions with no
/// registered codec id of its own; the value only appears in budget-exceeded
/// error messages on hostile input.
const OLLAMA_CODEC_ID: CodecId = CodecId(0);

/// Options controlling how a chat model-request transcript is projected into an
/// Ollama JSON request body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OllamaRequestOptions {
    /// The Ollama model name to send in the request.
    pub model: String,
    /// Whether to request a streamed response.
    pub stream: bool,
    /// Whether to include a (currently empty) `tools` array in the request.
    pub tools: bool,
}

impl OllamaRequestOptions {
    /// Creates request options from a `model` name and the `stream`/`tools`
    /// flags.
    pub fn new(model: impl Into<String>, stream: bool, tools: bool) -> Self {
        Self {
            model: model.into(),
            stream,
            tools,
        }
    }
}

/// Encodes a chat model-request transcript into an Ollama JSON request body.
///
/// Fails closed unless `expr` is a valid `model-request` transcript: prior
/// messages plus the request `task` are flattened into Ollama `messages`, and
/// the `model`/`stream`/`tools` options from `options` are applied.
///
/// # Examples
///
/// ```
/// use sim_codec_chat::{encode_ollama_request, model_card_expr, OllamaRequestOptions};
/// use sim_kernel::Symbol;
///
/// // A model-card is not a model-request, so the codec fails closed.
/// let card = model_card_expr(
///     Symbol::new("local-reasoner"),
///     "qwen2.5-coder:14b",
///     Symbol::new("ollama"),
///     Symbol::new("local"),
/// );
/// let options = OllamaRequestOptions::new("qwen2.5-coder:14b", false, false);
/// assert!(encode_ollama_request(&card, &options).is_err());
/// ```
pub fn encode_ollama_request(expr: &Expr, options: &OllamaRequestOptions) -> Result<Vec<u8>> {
    if !is_model_request_expr(expr) {
        return Err(Error::Eval(
            "ollama codec expects a model-request transcript".to_owned(),
        ));
    }
    validate_chat_transcript(expr)?;
    let mut payload = Map::new();
    payload.insert("model".to_owned(), Value::String(options.model.clone()));
    payload.insert("stream".to_owned(), Value::Bool(options.stream));
    payload.insert(
        "messages".to_owned(),
        Value::Array(transcript_messages(expr)?),
    );
    if options.tools {
        payload.insert("tools".to_owned(), Value::Array(Vec::new()));
    }
    serde_json::to_vec(&Value::Object(payload))
        .map_err(|err| Error::Eval(format!("ollama codec failed to encode request: {err}")))
}

/// Decodes a non-streamed Ollama JSON response `body` into a canonical
/// model-response transcript attributed to `runner` and `model`.
///
/// Extracts the response text and any token-usage counts; when `include_raw`
/// is set, the original JSON is also attached as a `raw-provider-response`
/// field.
pub fn decode_ollama_response(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    budget.check_input_bytes(OLLAMA_CODEC_ID, body.len())?;
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| Error::Eval(format!("ollama codec returned invalid json: {err}")))?;
    response_expr_from_json(runner, model, &value, include_raw, &mut budget)
}

/// Decodes a newline-delimited Ollama streaming response `body` into a single
/// canonical model-response transcript attributed to `runner` and `model`.
///
/// Concatenates the text of every chunk, derives the stop reason from the final
/// chunk, and folds in token-usage counts; when `include_raw` is set, the raw
/// chunks are attached as a `raw-provider-response` list. Errors if no chunks
/// are present.
pub fn decode_ollama_stream(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    budget.check_input_bytes(OLLAMA_CODEC_ID, body.len())?;
    let text = std::str::from_utf8(body)
        .map_err(|err| Error::Eval(format!("ollama stream is not valid utf-8: {err}")))?;
    let mut chunks = Vec::new();
    let mut combined = String::new();
    let mut usage_source = None;
    let mut stop_reason = Symbol::new("stop");
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed)
            .map_err(|err| Error::Eval(format!("ollama stream returned invalid json: {err}")))?;
        combined.push_str(&response_chunk_text(&value)?);
        if usage_expr_from_value(&value)?.is_some() {
            usage_source = Some(value.clone());
        }
        if let Some(reason) = value.get("done_reason").and_then(Value::as_str) {
            stop_reason = Symbol::new(reason);
        } else if value.get("done").and_then(Value::as_bool).unwrap_or(false) {
            stop_reason = Symbol::new("stop");
        }
        chunks.push(value);
    }
    if chunks.is_empty() {
        return Err(Error::Eval(
            "ollama stream did not contain any response chunks".to_owned(),
        ));
    }
    let mut entries =
        match model_response_expr(runner, model, vec![text_part(&combined)], stop_reason) {
            Expr::Map(entries) => entries,
            _ => unreachable!("model_response_expr always returns a map"),
        };
    if let Some(source) = usage_source.as_ref()
        && let Some(usage) = usage_expr_from_value(source)?
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
                    OLLAMA_CODEC_ID,
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

fn response_expr_from_json(
    runner: Symbol,
    model: &str,
    value: &Value,
    include_raw: bool,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let response = value
        .as_object()
        .ok_or_else(|| Error::Eval("ollama response must be a json object".to_owned()))?;
    let content = response_content(response)?;
    let stop_reason = response
        .get("done_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let mut entries = match model_response_expr(
        runner,
        model,
        vec![text_part(&content)],
        Symbol::new(stop_reason),
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
            project_json_to_expr_budgeted(
                value,
                JsonProjectionMode::UntaggedInterop,
                OLLAMA_CODEC_ID,
                budget,
                0,
            )?,
        ));
    }
    Ok(Expr::Map(entries))
}

fn transcript_messages(expr: &Expr) -> Result<Vec<Value>> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "ollama codec expects request transcript as a map".to_owned(),
        ));
    };
    let mut messages = access::entry_required_list_any(entries, "messages", "ollama messages")?
        .iter()
        .map(message_to_json)
        .collect::<Result<Vec<_>>>()?;
    messages.push(json!({
        "role": "user",
        "content": flatten_expr(map_field(entries, "task")?),
    }));
    Ok(messages)
}

fn message_to_json(expr: &Expr) -> Result<Value> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval("ollama codec message must be a map".to_owned()));
    };
    let role = access::entry_required_sym_any(entries, "role", "ollama message role")?;
    let content = access::entry_required_list_any(entries, "content", "ollama message content")?
        .iter()
        .map(content_part_to_text)
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    Ok(json!({
        "role": role.name.as_ref(),
        "content": content,
    }))
}

fn content_part_to_text(expr: &Expr) -> Result<String> {
    let Expr::Map(entries) = expr else {
        return Err(Error::Eval(
            "ollama codec content part must be a map".to_owned(),
        ));
    };
    match access::entry_required_sym_any(entries, "type", "ollama content part type")?
        .name
        .as_ref()
    {
        "text" => Ok(
            access::entry_required_str_any(entries, "text", "ollama content part text")?.to_owned(),
        ),
        other => Err(Error::Eval(format!(
            "ollama codec does not support content part type {other}"
        ))),
    }
}

fn response_content(response: &Map<String, Value>) -> Result<String> {
    if let Some(content) = response
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        return Ok(content.to_owned());
    }
    if let Some(content) = response.get("response").and_then(Value::as_str) {
        return Ok(content.to_owned());
    }
    Err(Error::Eval(
        "ollama response missing message.content or response".to_owned(),
    ))
}

fn response_chunk_text(value: &Value) -> Result<String> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Eval("ollama stream chunk must be a json object".to_owned()))?;
    if let Some(content) = object
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        return Ok(content.to_owned());
    }
    if let Some(content) = object.get("response").and_then(Value::as_str) {
        return Ok(content.to_owned());
    }
    Ok(String::new())
}

fn usage_expr_from_value(value: &Value) -> Result<Option<Expr>> {
    let response = value
        .as_object()
        .ok_or_else(|| Error::Eval("ollama usage source must be a json object".to_owned()))?;
    let input = response
        .get("prompt_eval_count")
        .and_then(json_number_to_u64);
    let output = response.get("eval_count").and_then(json_number_to_u64);
    // Ollama reports no total; it is derived only when both counts are present.
    // Saturate rather than wrap so a hostile body cannot overflow the u64 add.
    let total = input
        .zip(output)
        .map(|(input, output)| input.saturating_add(output));
    let fields = usage_record(input, output, total);
    Ok((!fields.is_empty()).then_some(Expr::Map(fields)))
}

fn map_field<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Result<&'a Expr> {
    entries
        .iter()
        .find_map(|(field, value)| match field {
            Expr::Symbol(symbol) if symbol.name.as_ref() == key => Some(value),
            _ => None,
        })
        .ok_or_else(|| Error::Eval(format!("ollama codec missing {key} field")))
}

fn flatten_expr(expr: &Expr) -> String {
    match expr {
        Expr::Nil => "nil".to_owned(),
        Expr::Bool(flag) => flag.to_string(),
        Expr::Number(number) => number.canonical.clone(),
        Expr::Symbol(symbol) | Expr::Local(symbol) => symbol.to_string(),
        Expr::String(text) => text.clone(),
        Expr::Bytes(bytes) => format!("{bytes:?}"),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.iter().map(flatten_expr).collect::<Vec<_>>().join(" ")
        }
        Expr::Map(entries) => entries
            .iter()
            .map(|(key, value)| format!("{} {}", flatten_expr(key), flatten_expr(value)))
            .collect::<Vec<_>>()
            .join(" "),
        Expr::Call { operator, args } => std::iter::once(flatten_expr(operator))
            .chain(args.iter().map(flatten_expr))
            .collect::<Vec<_>>()
            .join(" "),
        Expr::Infix {
            operator,
            left,
            right,
        } => format!(
            "{} {} {}",
            flatten_expr(left),
            operator,
            flatten_expr(right)
        ),
        Expr::Prefix { operator, arg } => format!("{operator} {}", flatten_expr(arg)),
        Expr::Postfix { operator, arg } => format!("{} {operator}", flatten_expr(arg)),
        Expr::Quote { expr, .. } | Expr::Annotated { expr, .. } => flatten_expr(expr),
        Expr::Extension { tag, payload } => format!("{tag} {}", flatten_expr(payload)),
    }
}
