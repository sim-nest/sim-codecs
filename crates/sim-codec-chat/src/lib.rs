//! Canonical chat transcript codec for SIM.
//!
//! This crate provides `codec:chat`, a provider-neutral text format for
//! model request, response, event, and card transcripts represented as
//! `Expr::Map` values.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod base64;
mod canonical;
mod expr;
mod helpers;
mod ollama;
mod parts;

#[cfg(test)]
mod tests;

pub use canonical::{ChatCodec, ChatCodecLib};
pub use helpers::{
    is_model_request_expr, model_card_expr, model_error_expr, model_request_messages_expr,
    model_response_expr, validate_chat_transcript,
};
pub use ollama::{
    OllamaRequestOptions, decode_ollama_response, decode_ollama_stream, encode_ollama_request,
};
pub use parts::{number_field, text_part, usage_record};
