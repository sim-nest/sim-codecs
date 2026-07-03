//! The MCP envelope model: the `McpEnvelope` enum and its request,
//! notification, response, and error payload structs, with their stable class
//! symbols.

use sim_citizen::CitizenField;
use sim_citizen_derive::Citizen;
use sim_kernel::Expr;
use sim_kernel::{Result, Symbol};

/// One MCP JSON-RPC 2.0 envelope: the typed payload carried by a single frame.
///
/// The four variants are the complete envelope shapes the `codec:mcp` codec
/// round-trips; any other JSON structure is rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum McpEnvelope {
    /// A method call with an `id` expecting a response.
    Request(McpRequest),
    /// A method call without an `id`, expecting no response.
    Notification(McpNotification),
    /// A successful result for a prior request `id`.
    Response(McpResponse),
    /// A failure for a prior request `id`.
    Error(McpErrorEnvelope),
}

/// A JSON-RPC request: an addressed method call expecting a response.
#[derive(Clone, Debug, PartialEq, Citizen)]
#[citizen(symbol = "mcp/Request", version = 1)]
pub struct McpRequest {
    /// The correlation id (string, number, or nil) the response must echo.
    pub id: Expr,
    /// The method name being invoked.
    pub method: String,
    /// The method parameters.
    pub params: Expr,
}

/// A JSON-RPC notification: a method call with no `id` and no response.
#[derive(Clone, Debug, PartialEq, Citizen)]
#[citizen(symbol = "mcp/Notification", version = 1)]
pub struct McpNotification {
    /// The method name being notified.
    pub method: String,
    /// The method parameters.
    pub params: Expr,
}

/// A JSON-RPC successful response for a prior request.
#[derive(Clone, Debug, PartialEq, Citizen)]
#[citizen(symbol = "mcp/Response", version = 1)]
pub struct McpResponse {
    /// The request id this response answers.
    pub id: Expr,
    /// The successful result payload.
    pub result: Expr,
}

/// A JSON-RPC error response for a prior request.
#[derive(Clone, Debug, PartialEq, Citizen)]
#[citizen(symbol = "mcp/ErrorEnvelope", version = 1)]
pub struct McpErrorEnvelope {
    /// The request id this error answers.
    pub id: Expr,
    /// The error detail.
    pub error: McpError,
}

/// The error object inside an [`McpErrorEnvelope`].
#[derive(Clone, Debug, PartialEq, Citizen)]
#[citizen(symbol = "mcp/Error", version = 1)]
pub struct McpError {
    /// The numeric error code (see the constants in this crate, e.g.
    /// [`INTERNAL_ERROR`](crate::INTERNAL_ERROR)).
    pub code: i64,
    /// A human-readable error message.
    pub message: String,
    /// Optional structured error data.
    pub data: Expr,
}

impl Default for McpRequest {
    fn default() -> Self {
        Self {
            id: Expr::String("fixture".to_owned()),
            method: "tools/list".to_owned(),
            params: Expr::Map(Vec::new()),
        }
    }
}

impl Default for McpNotification {
    fn default() -> Self {
        Self {
            method: "notifications/initialized".to_owned(),
            params: Expr::Map(Vec::new()),
        }
    }
}

impl Default for McpResponse {
    fn default() -> Self {
        Self {
            id: Expr::String("fixture".to_owned()),
            result: Expr::Map(Vec::new()),
        }
    }
}

impl Default for McpErrorEnvelope {
    fn default() -> Self {
        Self {
            id: Expr::String("fixture".to_owned()),
            error: McpError::default(),
        }
    }
}

impl Default for McpError {
    fn default() -> Self {
        Self {
            code: -32603,
            message: "fixture error".to_owned(),
            data: Expr::Nil,
        }
    }
}

impl CitizenField for McpError {
    fn encode_field(&self) -> Expr {
        Expr::List(vec![
            self.code.encode_field(),
            self.message.encode_field(),
            self.data.encode_field(),
        ])
    }

    fn decode_field_expr(expr: &Expr, field: &'static str) -> Result<Self> {
        let Expr::List(items) = expr else {
            return Err(sim_citizen::field_error(field, "expected MCP error list"));
        };
        let [code, message, data] = items.as_slice() else {
            return Err(sim_citizen::field_error(
                field,
                format!("expected 3 MCP error field(s), found {}", items.len()),
            ));
        };
        Ok(Self {
            code: i64::decode_field_expr(code, field)?,
            message: String::decode_field_expr(message, field)?,
            data: Expr::decode_field_expr(data, field)?,
        })
    }
}

/// The stable class symbol `mcp/Request` for [`McpRequest`].
pub fn mcp_request_class_symbol() -> Symbol {
    Symbol::qualified("mcp", "Request")
}

/// The stable class symbol `mcp/Notification` for [`McpNotification`].
pub fn mcp_notification_class_symbol() -> Symbol {
    Symbol::qualified("mcp", "Notification")
}

/// The stable class symbol `mcp/Response` for [`McpResponse`].
pub fn mcp_response_class_symbol() -> Symbol {
    Symbol::qualified("mcp", "Response")
}

/// The stable class symbol `mcp/ErrorEnvelope` for [`McpErrorEnvelope`].
pub fn mcp_error_envelope_class_symbol() -> Symbol {
    Symbol::qualified("mcp", "ErrorEnvelope")
}

/// The stable class symbol `mcp/Error` for [`McpError`].
pub fn mcp_error_class_symbol() -> Symbol {
    Symbol::qualified("mcp", "Error")
}

pub(crate) fn is_jsonrpc_id(expr: &Expr) -> bool {
    matches!(expr, Expr::String(_) | Expr::Number(_) | Expr::Nil)
}
