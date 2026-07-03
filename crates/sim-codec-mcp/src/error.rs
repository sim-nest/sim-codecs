//! The standard JSON-RPC and MCP error-code constants and a small helper for
//! building codec errors.

use sim_kernel::{CodecId, Error};

/// JSON-RPC parse error: invalid JSON was received (`-32700`).
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC invalid request: the payload is not a valid request object
/// (`-32600`).
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC method not found: the requested method does not exist (`-32601`).
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC invalid params: the method parameters are invalid (`-32602`).
pub const INVALID_PARAMS: i64 = -32602;
/// JSON-RPC internal error: an internal failure occurred (`-32603`).
pub const INTERNAL_ERROR: i64 = -32603;
/// MCP capability denied: a required capability was not granted (`-32001`).
pub const CAPABILITY_DENIED: i64 = -32001;
/// MCP execution error: a tool or method failed while executing (`-32002`).
pub const EXECUTION_ERROR: i64 = -32002;
/// MCP cancelled: the request was cancelled before completion (`-32003`).
pub const CANCELLED: i64 = -32003;
/// MCP not found: the addressed resource does not exist (`-32004`).
pub const NOT_FOUND: i64 = -32004;
/// MCP rate limited: the request was throttled (`-32005`).
pub const RATE_LIMITED: i64 = -32005;

pub(crate) fn codec_error(codec: CodecId, message: impl Into<String>) -> Error {
    Error::CodecError {
        codec,
        message: message.into(),
    }
}
