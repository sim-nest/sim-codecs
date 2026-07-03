//! Conversion between MCP envelopes and checked `Expr` values:
//! `envelope_to_expr` projects an envelope to its canonical map, and
//! `expr_to_envelope` validates a map back into a typed envelope.

use std::collections::BTreeSet;

use sim_kernel::{Error, Expr, NumberLiteral, Result, Symbol};

use crate::envelope::{
    McpEnvelope, McpError, McpErrorEnvelope, McpNotification, McpRequest, McpResponse,
    is_jsonrpc_id,
};

const MCP_VERSION: &str = "2.0";

/// Project an [`McpEnvelope`] into its canonical `Expr` map, with the `mcp`
/// version field and the variant-specific fields.
///
/// # Examples
///
/// ```
/// use sim_codec_mcp::{McpEnvelope, McpRequest, envelope_to_expr};
///
/// let envelope = McpEnvelope::Request(McpRequest::default());
/// let expr = envelope_to_expr(&envelope);
/// // Round-trips back to the same typed envelope.
/// assert_eq!(sim_codec_mcp::expr_to_envelope(&expr).unwrap(), envelope);
/// ```
pub fn envelope_to_expr(envelope: &McpEnvelope) -> Expr {
    match envelope {
        McpEnvelope::Request(request) => Expr::Map(vec![
            field("mcp", Expr::String(MCP_VERSION.to_owned())),
            field("id", request.id.clone()),
            field("method", Expr::String(request.method.clone())),
            field("params", request.params.clone()),
        ]),
        McpEnvelope::Notification(notification) => Expr::Map(vec![
            field("mcp", Expr::String(MCP_VERSION.to_owned())),
            field("method", Expr::String(notification.method.clone())),
            field("params", notification.params.clone()),
        ]),
        McpEnvelope::Response(response) => Expr::Map(vec![
            field("mcp", Expr::String(MCP_VERSION.to_owned())),
            field("id", response.id.clone()),
            field("result", response.result.clone()),
        ]),
        McpEnvelope::Error(error) => Expr::Map(vec![
            field("mcp", Expr::String(MCP_VERSION.to_owned())),
            field("id", error.id.clone()),
            field(
                "error",
                Expr::Map(vec![
                    field("code", error_code_expr(error.error.code)),
                    field("message", Expr::String(error.error.message.clone())),
                    field("data", error.error.data.clone()),
                ]),
            ),
        ]),
    }
}

/// Validate a canonical `Expr` map back into a typed [`McpEnvelope`].
///
/// The map must declare `mcp: "2.0"` and exactly the field set of one envelope
/// variant; unknown, duplicate, or mismatched fields are rejected, so the
/// codec fails closed on non-MCP input.
///
/// # Examples
///
/// ```
/// use sim_codec_mcp::{McpEnvelope, McpResponse, envelope_to_expr, expr_to_envelope};
///
/// let expr = envelope_to_expr(&McpEnvelope::Response(McpResponse::default()));
/// assert!(matches!(expr_to_envelope(&expr).unwrap(), McpEnvelope::Response(_)));
/// ```
pub fn expr_to_envelope(expr: &Expr) -> Result<McpEnvelope> {
    let fields = map_fields(expr, "MCP envelope")?;
    reject_unknown(
        fields,
        &["mcp", "id", "method", "params", "result", "error"],
    )?;
    require_version(fields)?;

    let has_id = optional_field(fields, "id").is_some();
    let has_method = optional_field(fields, "method").is_some();
    let has_result = optional_field(fields, "result").is_some();
    let has_error = optional_field(fields, "error").is_some();

    match (has_method, has_id, has_result, has_error) {
        (true, true, false, false) => request_from_fields(fields),
        (true, false, false, false) => notification_from_fields(fields),
        (false, true, true, false) => response_from_fields(fields),
        (false, true, false, true) => error_from_fields(fields),
        _ => Err(Error::Eval(
            "invalid MCP JSON-RPC envelope field combination".to_owned(),
        )),
    }
}

fn request_from_fields(fields: &[(Expr, Expr)]) -> Result<McpEnvelope> {
    reject_unknown(fields, &["mcp", "id", "method", "params"])?;
    let id = required_id(fields)?;
    let method = required_string(fields, "method")?;
    let params = required_field(fields, "params")?.clone();
    Ok(McpEnvelope::Request(McpRequest { id, method, params }))
}

fn notification_from_fields(fields: &[(Expr, Expr)]) -> Result<McpEnvelope> {
    reject_unknown(fields, &["mcp", "method", "params"])?;
    let method = required_string(fields, "method")?;
    let params = required_field(fields, "params")?.clone();
    Ok(McpEnvelope::Notification(McpNotification {
        method,
        params,
    }))
}

fn response_from_fields(fields: &[(Expr, Expr)]) -> Result<McpEnvelope> {
    reject_unknown(fields, &["mcp", "id", "result"])?;
    let id = required_id(fields)?;
    let result = required_field(fields, "result")?.clone();
    Ok(McpEnvelope::Response(McpResponse { id, result }))
}

fn error_from_fields(fields: &[(Expr, Expr)]) -> Result<McpEnvelope> {
    reject_unknown(fields, &["mcp", "id", "error"])?;
    let id = required_id(fields)?;
    let error = error_object(required_field(fields, "error")?)?;
    Ok(McpEnvelope::Error(McpErrorEnvelope { id, error }))
}

fn error_object(expr: &Expr) -> Result<McpError> {
    let fields = map_fields(expr, "MCP error object")?;
    reject_unknown(fields, &["code", "message", "data"])?;
    Ok(McpError {
        code: required_i64(fields, "code")?,
        message: required_string(fields, "message")?,
        data: required_field(fields, "data")?.clone(),
    })
}

fn require_version(fields: &[(Expr, Expr)]) -> Result<()> {
    match required_field(fields, "mcp")? {
        Expr::String(version) if version == MCP_VERSION => Ok(()),
        _ => Err(Error::Eval(
            "MCP envelope must declare :mcp \"2.0\"".to_owned(),
        )),
    }
}

fn required_id(fields: &[(Expr, Expr)]) -> Result<Expr> {
    let id = required_field(fields, "id")?.clone();
    if is_jsonrpc_id(&id) {
        Ok(id)
    } else {
        Err(Error::TypeMismatch {
            expected: "JSON-RPC id string, number, or nil",
            found: "invalid id",
        })
    }
}

fn required_i64(fields: &[(Expr, Expr)], name: &str) -> Result<i64> {
    match required_field(fields, name)? {
        Expr::Number(number) => number
            .canonical
            .parse::<i64>()
            .map_err(|_| Error::TypeMismatch {
                expected: "integer error code",
                found: "non-integer number",
            }),
        _ => Err(Error::TypeMismatch {
            expected: "integer error code",
            found: "non-number",
        }),
    }
}

fn required_string(fields: &[(Expr, Expr)], name: &str) -> Result<String> {
    match required_field(fields, name)? {
        Expr::String(value) => Ok(value.clone()),
        _ => Err(Error::TypeMismatch {
            expected: "string",
            found: "non-string",
        }),
    }
}

fn required_field<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a Expr> {
    optional_field(fields, name)
        .ok_or_else(|| Error::Eval(format!("MCP envelope is missing {name}")))
}

fn optional_field<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    fields
        .iter()
        .find_map(|(key, value)| (field_name(key).ok()?.as_str() == name).then_some(value))
}

use sim_value::access::map_entries as map_fields;

fn reject_unknown(fields: &[(Expr, Expr)], allowed: &[&str]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (key, _) in fields {
        let name = field_name(key)?;
        if !seen.insert(name.clone()) {
            return Err(Error::Eval(format!("duplicate MCP envelope field {name}")));
        }
        if !allowed.contains(&name.as_str()) {
            return Err(Error::Eval(format!("unknown MCP envelope field {name}")));
        }
    }
    Ok(())
}

fn field_name(expr: &Expr) -> Result<String> {
    match expr {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Ok(symbol.name.to_string()),
        Expr::String(value) => Ok(value.clone()),
        _ => Err(Error::TypeMismatch {
            expected: "MCP envelope field symbol",
            found: "invalid field key",
        }),
    }
}

fn field(name: &str, value: Expr) -> (Expr, Expr) {
    sim_value::build::entry(name, value)
}

pub(crate) fn error_code_expr(code: i64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: code.to_string(),
    })
}
