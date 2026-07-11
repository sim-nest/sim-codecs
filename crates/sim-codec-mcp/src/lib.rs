//! MCP JSON-RPC envelope codec for SIM.
//!
//! This crate provides `codec:mcp`, a pure data codec for one MCP JSON-RPC
//! envelope per frame. It owns only envelope validation and conversion; routing,
//! transport, and callable execution live in later MCP libraries. As a domain
//! codec it round-trips only MCP envelopes and fails closed outside them.
//!
//! Module map (all modules are private; the public surface is re-exported from
//! this crate root):
//! - canonical: the `McpCodec` decoder/encoder and the `McpCodecLib` host lib
//!   bridging MCP JSON-RPC text and the envelope model.
//! - envelope: the envelope types (`McpEnvelope` and its request, notification,
//!   response, and error variants) and their class symbols.
//! - error: the JSON-RPC and MCP error-code constants and the codec error
//!   helper.
//! - expr: conversion between envelopes and checked `Expr` values
//!   (`envelope_to_expr`, `expr_to_envelope`).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod canonical;
mod envelope;
mod error;
mod expr;

#[cfg(test)]
mod tests;

pub use canonical::{McpCodec, McpCodecLib};
pub use envelope::{
    McpEnvelope, McpError, McpErrorEnvelope, McpNotification, McpRequest, McpResponse,
    mcp_error_class_symbol, mcp_error_envelope_class_symbol, mcp_notification_class_symbol,
    mcp_request_class_symbol, mcp_response_class_symbol,
};
pub use error::{
    CANCELLED, CAPABILITY_DENIED, EXECUTION_ERROR, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST,
    METHOD_NOT_FOUND, NOT_FOUND, PARSE_ERROR, RATE_LIMITED,
};
pub use expr::{envelope_to_expr, expr_to_envelope};

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
