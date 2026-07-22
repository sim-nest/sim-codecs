//! Provider-specific chat wire codecs built on the canonical transcript shape.
//!
//! The provider modules translate hosted and local model-provider JSON into
//! the same `codec:chat` transcript maps instead of making each runner invent
//! its own request and response records.

/// Anthropic Messages provider wire codec.
pub mod anthropic;
/// Lemonade OpenAI-compatible provider wire codec.
pub mod lemonade;
/// LM Studio OpenAI-compatible provider wire codec.
pub mod lm_studio;
/// OpenAI-compatible chat-completion provider wire codec.
pub mod openai;
/// Shared helpers for OpenAI-compatible provider wire codecs.
pub(in crate::providers) mod openai_compat;
/// Open provider profile records used by runners and browse surfaces.
pub mod profile;

/// Ollama provider wire helpers, also preserved at the crate root.
pub mod ollama {
    pub use crate::ollama::{
        OllamaCodec, OllamaCodecLib, OllamaRequestOptions, decode_ollama_response,
        decode_ollama_response_with_limits, decode_ollama_stream, decode_ollama_stream_with_limits,
        encode_ollama_request,
    };
}
