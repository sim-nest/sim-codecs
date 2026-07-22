use std::collections::BTreeMap;

use serde_json::{Map, Value};
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_codec_json::{JsonProjectionMode, project_json_to_expr_budgeted};
use sim_kernel::{Error, Expr, Result, Symbol};

use crate::{model_response_expr, text_part};

use super::super::ANTHROPIC_CODEC_ID;
use super::super::common::provider_symbol;
use super::shared::{
    error_message, error_response_expr, event_response, final_event_expr, model_event_expr,
    raw_provider_expr, string_member, usage_expr_from_value,
};

/// Decodes an Anthropic Messages SSE body into a final model-response
/// transcript, optionally embedding the raw provider chunks.
pub fn decode_anthropic_stream(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    let events = decode_anthropic_stream_events_with_limits(
        runner,
        model,
        body,
        include_raw,
        DecodeLimits::default(),
    )?;
    events
        .iter()
        .rev()
        .find_map(event_response)
        .cloned()
        .ok_or_else(|| Error::Eval("anthropic stream did not produce a final response".to_owned()))
}

/// Decodes an Anthropic Messages SSE body into model-event transcripts.
///
/// Text deltas are emitted as `model-event` records, tool-use blocks emit a
/// `tool-call` event, and the terminal event carries the final
/// `model-response` transcript.
pub fn decode_anthropic_stream_events(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Vec<Expr>> {
    decode_anthropic_stream_events_with_limits(
        runner,
        model,
        body,
        include_raw,
        DecodeLimits::default(),
    )
}

/// Decodes an Anthropic Messages SSE body into the final response under limits.
pub fn decode_anthropic_stream_with_limits(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
    limits: DecodeLimits,
) -> Result<Expr> {
    let events =
        decode_anthropic_stream_events_with_limits(runner, model, body, include_raw, limits)?;
    events
        .iter()
        .rev()
        .find_map(event_response)
        .cloned()
        .ok_or_else(|| Error::Eval("anthropic stream did not produce a final response".to_owned()))
}

/// Decodes Anthropic Messages SSE events under caller-supplied limits.
pub fn decode_anthropic_stream_events_with_limits(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
    limits: DecodeLimits,
) -> Result<Vec<Expr>> {
    let mut budget = DecodeBudget::new(limits);
    budget.check_input_bytes(ANTHROPIC_CODEC_ID, body.len())?;
    let text = std::str::from_utf8(body)
        .map_err(|err| Error::Eval(format!("anthropic stream is not valid utf-8: {err}")))?;
    let sse_events = parse_sse_events(text);
    if sse_events.is_empty() {
        return Err(Error::Eval(
            "anthropic stream did not contain any response chunks".to_owned(),
        ));
    }
    budget.check_collection_len(ANTHROPIC_CODEC_ID, sse_events.len())?;
    let mut events = Vec::new();
    let mut response_model = model.to_owned();
    let mut span_id = Expr::Symbol(Symbol::new("stream"));
    let mut blocks: BTreeMap<usize, StreamBlock> = BTreeMap::new();
    let mut usage = None;
    let mut stop_reason = Symbol::new("stop");
    let mut raw_chunks = Vec::new();
    for event in sse_events {
        let value: Value = serde_json::from_str(&event.data)
            .map_err(|err| Error::Eval(format!("anthropic stream returned invalid json: {err}")))?;
        if include_raw {
            raw_chunks.push(value.clone());
        }
        if let Some(error) = error_message(value.as_object()) {
            let response = error_response_expr(
                runner.clone(),
                model,
                error,
                include_raw,
                Some(&value),
                &mut budget,
            )?;
            events.push(final_event_expr(
                runner.clone(),
                model,
                span_id.clone(),
                response,
            ));
            return Ok(events);
        }
        match event.name.as_str() {
            "message_start" => {
                if let Some(message) = value.get("message").and_then(Value::as_object) {
                    if let Some(id) = message.get("id").and_then(Value::as_str) {
                        span_id = Expr::String(id.to_owned());
                    }
                    if let Some(found_model) = message.get("model").and_then(Value::as_str) {
                        response_model = found_model.to_owned();
                    }
                }
                events.push(model_event_expr(
                    "start",
                    runner.clone(),
                    &response_model,
                    span_id.clone(),
                    Vec::new(),
                ));
            }
            "content_block_start" => {
                let index = event_index(&value)?;
                let block = value
                    .get("content_block")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        Error::Eval(
                            "anthropic content_block_start missing content_block".to_owned(),
                        )
                    })?;
                blocks.insert(index, stream_block_from_start(block)?);
            }
            "content_block_delta" => {
                let index = event_index(&value)?;
                let delta = value
                    .get("delta")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        Error::Eval("anthropic content_block_delta missing delta".to_owned())
                    })?;
                handle_delta(
                    delta,
                    index,
                    &mut blocks,
                    &mut events,
                    runner.clone(),
                    &response_model,
                    span_id.clone(),
                )?;
            }
            "content_block_stop" => {
                let index = event_index(&value)?;
                if let Some(tool_call) = blocks
                    .get_mut(&index)
                    .map(|block| block.finish_tool_call(&mut budget))
                    .transpose()?
                    .flatten()
                {
                    events.push(model_event_expr(
                        "tool-call",
                        runner.clone(),
                        &response_model,
                        span_id.clone(),
                        vec![(Expr::Symbol(Symbol::new("tool-call")), tool_call)],
                    ));
                }
            }
            "message_delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_object)
                    && let Some(reason) = delta.get("stop_reason").and_then(Value::as_str)
                {
                    stop_reason = provider_symbol(reason);
                }
                if let Some(found) = usage_expr_from_value(&value)? {
                    usage = Some(found.clone());
                    events.push(model_event_expr(
                        "usage",
                        runner.clone(),
                        &response_model,
                        span_id.clone(),
                        vec![(Expr::Symbol(Symbol::new("usage")), found)],
                    ));
                }
            }
            "message_stop" => {
                let response = stream_response_expr(
                    runner.clone(),
                    &response_model,
                    &blocks,
                    stop_reason.clone(),
                    usage.clone(),
                    include_raw.then_some(raw_chunks.as_slice()),
                    &mut budget,
                )?;
                events.push(final_event_expr(
                    runner.clone(),
                    &response_model,
                    span_id.clone(),
                    response,
                ));
            }
            "ping" => {}
            _ => {}
        }
    }
    if !events.iter().any(|event| event_response(event).is_some()) {
        let response = stream_response_expr(
            runner.clone(),
            &response_model,
            &blocks,
            stop_reason,
            usage,
            include_raw.then_some(raw_chunks.as_slice()),
            &mut budget,
        )?;
        events.push(final_event_expr(runner, &response_model, span_id, response));
    }
    Ok(events)
}

fn handle_delta(
    delta: &Map<String, Value>,
    index: usize,
    blocks: &mut BTreeMap<usize, StreamBlock>,
    events: &mut Vec<Expr>,
    runner: Symbol,
    model: &str,
    span_id: Expr,
) -> Result<()> {
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => {
            let text = string_member(ANTHROPIC_CODEC_ID, delta, "text")?;
            blocks
                .entry(index)
                .or_insert_with(|| StreamBlock::Text(String::new()))
                .push_text(text)?;
            events.push(model_event_expr(
                "delta",
                runner,
                model,
                span_id,
                vec![(
                    Expr::Symbol(Symbol::new("text")),
                    Expr::String(text.to_owned()),
                )],
            ));
        }
        Some("input_json_delta") => {
            let partial = delta
                .get("partial_json")
                .and_then(Value::as_str)
                .unwrap_or("");
            blocks
                .entry(index)
                .or_insert_with(StreamBlock::empty_tool)
                .push_partial_json(partial)?;
        }
        Some("thinking_delta") => {
            if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                blocks
                    .entry(index)
                    .or_insert_with(|| StreamBlock::Text(String::new()))
                    .push_text(thinking)?;
            }
        }
        Some(_) | None => {}
    }
    Ok(())
}

fn stream_response_expr(
    runner: Symbol,
    model: &str,
    blocks: &BTreeMap<usize, StreamBlock>,
    stop_reason: Symbol,
    usage: Option<Expr>,
    raw_chunks: Option<&[Value]>,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let mut content = Vec::new();
    for block in blocks.values() {
        content.push(block.content_part(budget)?);
    }
    let mut entries = match model_response_expr(runner, model, content, stop_reason) {
        Expr::Map(entries) => entries,
        _ => unreachable!("model_response_expr always returns a map"),
    };
    if let Some(usage) = usage {
        entries.push((Expr::Symbol(Symbol::new("usage")), usage));
    }
    if let Some(raw_chunks) = raw_chunks {
        entries.push((
            Expr::Symbol(Symbol::new("raw-provider-response")),
            Expr::List(
                raw_chunks
                    .iter()
                    .map(|chunk| raw_provider_expr(chunk, budget))
                    .collect::<Result<Vec<_>>>()?,
            ),
        ));
    }
    Ok(Expr::Map(entries))
}

fn event_index(value: &Value) -> Result<usize> {
    let raw = value
        .get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Eval("anthropic stream event missing index".to_owned()))?;
    usize::try_from(raw)
        .map_err(|_| Error::Eval("anthropic stream event index too large".to_owned()))
}

#[derive(Clone, Debug)]
enum StreamBlock {
    Text(String),
    Tool {
        id: String,
        name: String,
        input: Value,
        partial_json: String,
    },
}

impl StreamBlock {
    fn empty_tool() -> Self {
        Self::Tool {
            id: "toolu_stream".to_owned(),
            name: "tool".to_owned(),
            input: Value::Object(Map::new()),
            partial_json: String::new(),
        }
    }

    fn push_text(&mut self, text: &str) -> Result<()> {
        match self {
            Self::Text(buffer) => {
                buffer.push_str(text);
                Ok(())
            }
            Self::Tool { .. } => Err(Error::Eval(
                "anthropic text delta arrived for tool-use block".to_owned(),
            )),
        }
    }

    fn push_partial_json(&mut self, partial: &str) -> Result<()> {
        match self {
            Self::Tool { partial_json, .. } => {
                partial_json.push_str(partial);
                Ok(())
            }
            Self::Text(_) => Err(Error::Eval(
                "anthropic input-json delta arrived for text block".to_owned(),
            )),
        }
    }

    fn finish_tool_call(&mut self, budget: &mut DecodeBudget) -> Result<Option<Expr>> {
        let Self::Tool {
            id,
            name,
            input,
            partial_json,
        } = self
        else {
            return Ok(None);
        };
        if !partial_json.trim().is_empty() {
            *input = serde_json::from_str(partial_json).map_err(|err| {
                Error::Eval(format!("anthropic tool-use partial json invalid: {err}"))
            })?;
            partial_json.clear();
        }
        Ok(Some(Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("type")),
                Expr::Symbol(Symbol::new("tool-call")),
            ),
            (Expr::Symbol(Symbol::new("id")), Expr::String(id.clone())),
            (
                Expr::Symbol(Symbol::new("name")),
                Expr::String(name.clone()),
            ),
            (
                Expr::Symbol(Symbol::new("arguments")),
                project_json_to_expr_budgeted(
                    input,
                    JsonProjectionMode::UntaggedInterop,
                    ANTHROPIC_CODEC_ID,
                    budget,
                    0,
                )?,
            ),
        ])))
    }

    fn content_part(&self, budget: &mut DecodeBudget) -> Result<Expr> {
        match self {
            Self::Text(text) => Ok(text_part(text)),
            Self::Tool {
                id, name, input, ..
            } => Ok(Expr::Map(vec![
                (
                    Expr::Symbol(Symbol::new("type")),
                    Expr::Symbol(Symbol::new("tool-call")),
                ),
                (Expr::Symbol(Symbol::new("id")), Expr::String(id.clone())),
                (
                    Expr::Symbol(Symbol::new("name")),
                    Expr::String(name.clone()),
                ),
                (
                    Expr::Symbol(Symbol::new("arguments")),
                    project_json_to_expr_budgeted(
                        input,
                        JsonProjectionMode::UntaggedInterop,
                        ANTHROPIC_CODEC_ID,
                        budget,
                        0,
                    )?,
                ),
            ])),
        }
    }
}

fn stream_block_from_start(object: &Map<String, Value>) -> Result<StreamBlock> {
    match object.get("type").and_then(Value::as_str).unwrap_or("text") {
        "text" => Ok(StreamBlock::Text(
            object
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        )),
        "tool_use" | "server_tool_use" => Ok(StreamBlock::Tool {
            id: object
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("toolu_stream")
                .to_owned(),
            name: object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned(),
            input: object
                .get("input")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new())),
            partial_json: String::new(),
        }),
        _ => Ok(StreamBlock::Text(String::new())),
    }
}

#[derive(Clone, Debug)]
struct SseEvent {
    name: String,
    data: String,
}

fn parse_sse_events(text: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut name = None;
    let mut data = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if !data.is_empty() {
                events.push(SseEvent {
                    name: name.take().unwrap_or_else(|| "message".to_owned()),
                    data: data.join("\n"),
                });
                data.clear();
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim().to_owned());
        }
    }
    if !data.is_empty() {
        events.push(SseEvent {
            name: name.unwrap_or_else(|| "message".to_owned()),
            data: data.join("\n"),
        });
    }
    events
}
