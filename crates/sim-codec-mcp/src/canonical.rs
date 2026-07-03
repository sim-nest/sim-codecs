//! The `codec:mcp` decoder/encoder and its host-registered lib. Decodes one MCP
//! JSON-RPC envelope per frame into an envelope `Expr` and encodes envelopes
//! back to JSON-RPC text, validating the envelope on both sides.

use std::{str::FromStr, sync::Arc};

use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use sim_codec::{DecodeBudget, Decoder, DomainCodecLib, Encoder, Input, Output, ReadCx};
use sim_kernel::{
    CodecId, Error, Expr, Lib, LibManifest, Linker, LoadCx, NumberLiteral, Result, Symbol, WriteCx,
};

use crate::{
    envelope::{
        McpEnvelope, McpError, McpErrorEnvelope, McpNotification, McpRequest, McpResponse,
        is_jsonrpc_id,
    },
    error::codec_error,
    expr::{envelope_to_expr, expr_to_envelope},
};

const JSONRPC_VERSION: &str = "2.0";

/// The `codec:mcp` decoder/encoder.
///
/// As a [`Decoder`] it parses one MCP JSON-RPC envelope per frame into a
/// canonical envelope `Expr`; as an [`Encoder`] it validates an envelope `Expr`
/// and writes it back to JSON-RPC text. Non-MCP JSON is rejected.
pub struct McpCodec;

impl Decoder for McpCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input_text(cx.codec, input)?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let value = serde_json::from_str::<JsonValue>(&source)
            .map_err(|err| codec_error(cx.codec, format!("MCP JSON parse error: {err}")))?;
        let envelope = json_to_envelope(cx.codec, &value, &mut budget)?;
        Ok(envelope_to_expr(&envelope))
    }
}

impl Encoder for McpCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        let envelope = expr_to_envelope(expr).map_err(|err| Error::CodecError {
            codec: cx.codec,
            message: err.to_string(),
        })?;
        let value = envelope_to_json(cx.codec, &envelope)?;
        serde_json::to_string(&value)
            .map(Output::Text)
            .map_err(|err| codec_error(cx.codec, err.to_string()))
    }
}

fn input_text(codec: CodecId, input: Input) -> Result<String> {
    match input {
        Input::Text(text) => Ok(text),
        Input::Bytes(bytes) => String::from_utf8(bytes)
            .map_err(|err| codec_error(codec, format!("MCP input is not valid UTF-8: {err}"))),
    }
}

fn json_to_envelope(
    codec: CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
) -> Result<McpEnvelope> {
    match value {
        JsonValue::Array(_) => Err(codec_error(
            codec,
            "MCP batch arrays are not supported by codec:mcp",
        )),
        JsonValue::Object(map) => json_object_to_envelope(codec, map, budget),
        _ => Err(codec_error(codec, "MCP envelope must be a JSON object")),
    }
}

fn json_object_to_envelope(
    codec: CodecId,
    map: &JsonMap<String, JsonValue>,
    budget: &mut DecodeBudget,
) -> Result<McpEnvelope> {
    require_jsonrpc(codec, map)?;
    let has_id = map.contains_key("id");
    let has_method = map.contains_key("method");
    let has_result = map.contains_key("result");
    let has_error = map.contains_key("error");

    match (has_method, has_id, has_result, has_error) {
        (true, true, false, false) => json_request(codec, map, budget),
        (true, false, false, false) => json_notification(codec, map, budget),
        (false, true, true, false) => json_response(codec, map, budget),
        (false, true, false, true) => json_error_response(codec, map, budget),
        _ => Err(codec_error(
            codec,
            "invalid MCP JSON-RPC envelope field combination",
        )),
    }
}

fn json_request(
    codec: CodecId,
    map: &JsonMap<String, JsonValue>,
    budget: &mut DecodeBudget,
) -> Result<McpEnvelope> {
    reject_unknown_json(codec, map, &["jsonrpc", "id", "method", "params"])?;
    Ok(McpEnvelope::Request(McpRequest {
        id: json_id(codec, required_json(codec, map, "id")?)?,
        method: required_json_string(codec, map, "method")?.to_owned(),
        params: json_value_expr(codec, map.get("params"), budget)?,
    }))
}

fn json_notification(
    codec: CodecId,
    map: &JsonMap<String, JsonValue>,
    budget: &mut DecodeBudget,
) -> Result<McpEnvelope> {
    reject_unknown_json(codec, map, &["jsonrpc", "method", "params"])?;
    Ok(McpEnvelope::Notification(McpNotification {
        method: required_json_string(codec, map, "method")?.to_owned(),
        params: json_value_expr(codec, map.get("params"), budget)?,
    }))
}

fn json_response(
    codec: CodecId,
    map: &JsonMap<String, JsonValue>,
    budget: &mut DecodeBudget,
) -> Result<McpEnvelope> {
    reject_unknown_json(codec, map, &["jsonrpc", "id", "result"])?;
    Ok(McpEnvelope::Response(McpResponse {
        id: json_id(codec, required_json(codec, map, "id")?)?,
        result: json_value_expr(codec, map.get("result"), budget)?,
    }))
}

fn json_error_response(
    codec: CodecId,
    map: &JsonMap<String, JsonValue>,
    budget: &mut DecodeBudget,
) -> Result<McpEnvelope> {
    reject_unknown_json(codec, map, &["jsonrpc", "id", "error"])?;
    Ok(McpEnvelope::Error(McpErrorEnvelope {
        id: json_id(codec, required_json(codec, map, "id")?)?,
        error: json_error_object(codec, required_json(codec, map, "error")?, budget)?,
    }))
}

fn json_error_object(
    codec: CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
) -> Result<McpError> {
    let JsonValue::Object(map) = value else {
        return Err(codec_error(codec, "MCP error must be an object"));
    };
    reject_unknown_json(codec, map, &["code", "message", "data"])?;
    let Some(code) = required_json(codec, map, "code")?.as_i64() else {
        return Err(codec_error(codec, "MCP error code must be an integer"));
    };
    Ok(McpError {
        code,
        message: required_json_string(codec, map, "message")?.to_owned(),
        data: json_value_expr(codec, map.get("data"), budget)?,
    })
}

fn json_value_expr(
    codec: CodecId,
    value: Option<&JsonValue>,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    match value {
        Some(value) => sim_codec_json::json_to_expr(codec, value, budget, 1),
        None => Ok(Expr::Nil),
    }
}

fn require_jsonrpc(codec: CodecId, map: &JsonMap<String, JsonValue>) -> Result<()> {
    match map.get("jsonrpc") {
        Some(JsonValue::String(version)) if version == JSONRPC_VERSION => Ok(()),
        _ => Err(codec_error(
            codec,
            "MCP JSON-RPC envelope must declare jsonrpc \"2.0\"",
        )),
    }
}

fn required_json<'a>(
    codec: CodecId,
    map: &'a JsonMap<String, JsonValue>,
    key: &str,
) -> Result<&'a JsonValue> {
    map.get(key)
        .ok_or_else(|| codec_error(codec, format!("MCP envelope is missing {key}")))
}

fn required_json_string<'a>(
    codec: CodecId,
    map: &'a JsonMap<String, JsonValue>,
    key: &str,
) -> Result<&'a str> {
    required_json(codec, map, key)?
        .as_str()
        .ok_or_else(|| codec_error(codec, format!("MCP envelope {key} must be a string")))
}

fn reject_unknown_json(
    codec: CodecId,
    map: &JsonMap<String, JsonValue>,
    allowed: &[&str],
) -> Result<()> {
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(codec_error(
                codec,
                format!("unknown MCP JSON-RPC field {key}"),
            ));
        }
    }
    Ok(())
}

fn json_id(codec: CodecId, value: &JsonValue) -> Result<Expr> {
    match value {
        JsonValue::String(text) => Ok(Expr::String(text.clone())),
        JsonValue::Number(number) => Ok(Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: number.to_string(),
        })),
        JsonValue::Null => Ok(Expr::Nil),
        _ => Err(codec_error(
            codec,
            "MCP JSON-RPC id must be a string, number, or null",
        )),
    }
}

fn envelope_to_json(codec: CodecId, envelope: &McpEnvelope) -> Result<JsonValue> {
    let mut map = JsonMap::new();
    map.insert(
        "jsonrpc".to_owned(),
        JsonValue::String(JSONRPC_VERSION.to_owned()),
    );
    match envelope {
        McpEnvelope::Request(request) => {
            map.insert("id".to_owned(), id_to_json(codec, &request.id)?);
            map.insert(
                "method".to_owned(),
                JsonValue::String(request.method.clone()),
            );
            map.insert(
                "params".to_owned(),
                sim_codec_json::expr_to_json(&request.params),
            );
        }
        McpEnvelope::Notification(notification) => {
            map.insert(
                "method".to_owned(),
                JsonValue::String(notification.method.clone()),
            );
            map.insert(
                "params".to_owned(),
                sim_codec_json::expr_to_json(&notification.params),
            );
        }
        McpEnvelope::Response(response) => {
            map.insert("id".to_owned(), id_to_json(codec, &response.id)?);
            map.insert(
                "result".to_owned(),
                sim_codec_json::expr_to_json(&response.result),
            );
        }
        McpEnvelope::Error(error) => {
            map.insert("id".to_owned(), id_to_json(codec, &error.id)?);
            map.insert(
                "error".to_owned(),
                JsonValue::Object(error_to_json(&error.error)),
            );
        }
    }
    Ok(JsonValue::Object(map))
}

fn error_to_json(error: &McpError) -> JsonMap<String, JsonValue> {
    let mut map = JsonMap::new();
    map.insert(
        "code".to_owned(),
        JsonValue::Number(JsonNumber::from(error.code)),
    );
    map.insert(
        "message".to_owned(),
        JsonValue::String(error.message.clone()),
    );
    map.insert("data".to_owned(), sim_codec_json::expr_to_json(&error.data));
    map
}

fn id_to_json(codec: CodecId, id: &Expr) -> Result<JsonValue> {
    if !is_jsonrpc_id(id) {
        return Err(codec_error(
            codec,
            "MCP JSON-RPC id must be a string, number, or nil",
        ));
    }
    match id {
        Expr::String(text) => Ok(JsonValue::String(text.clone())),
        Expr::Number(number) => JsonNumber::from_str(&number.canonical)
            .map(JsonValue::Number)
            .map_err(|err| codec_error(codec, format!("invalid MCP numeric id: {err}"))),
        Expr::Nil => Ok(JsonValue::Null),
        _ => unreachable!("validated MCP id variants above"),
    }
}

/// The host-registered [`Lib`] that installs [`McpCodec`] as the domain codec
/// `codec:mcp`.
pub struct McpCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl McpCodecLib {
    /// Create the lib bound to the given codec id (obtained from
    /// [`Registry::fresh_codec_id`](sim_kernel::Registry::fresh_codec_id)).
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "mcp"),
            codec_id: id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(McpCodec),
            Arc::new(McpCodec),
            Symbol::qualified("codec", "McpEnvelope"),
        )
    }
}

impl Lib for McpCodecLib {
    fn manifest(&self) -> LibManifest {
        self.domain_lib().manifest()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        self.domain_lib().load(cx, linker)
    }
}
