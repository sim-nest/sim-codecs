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
pub mod providers;

#[cfg(test)]
mod tests;

pub use canonical::{ChatCodec, ChatCodecLib};
pub use helpers::{
    is_model_request_expr, model_card_expr, model_error_expr, model_request_messages_expr,
    model_response_expr, validate_chat_transcript,
};
pub use ollama::{
    OllamaCodec, OllamaCodecLib, OllamaRequestOptions, decode_ollama_response,
    decode_ollama_stream, encode_ollama_request,
};
pub use parts::{number_field, text_part, usage_record};
pub use providers::openai::{
    OpenAiCodec, OpenAiCodecLib, OpenAiCodecOptions, OpenAiRequestOptions, decode_openai_request,
    decode_openai_response, decode_openai_stream, encode_openai_request, encode_openai_response,
    openai_codec_symbol,
};
pub use providers::profile::{
    CodecProfile, RequestWire, StreamWire, anthropic_profile, lemonade_profile, lm_studio_profile,
    ollama_profile, openai_profile,
};

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
