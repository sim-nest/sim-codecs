//! Provider-specific chat wire codecs built on the canonical transcript shape.
//!
//! The provider modules translate hosted and local model-provider JSON into
//! the same `codec:chat` transcript maps instead of making each runner invent
//! its own request and response records.

/// Anthropic Messages provider wire codec.
pub mod anthropic;
/// OpenAI-compatible chat-completion provider wire codec.
pub mod openai;
/// Open provider profile records used by runners and browse surfaces.
pub mod profile;

/// Ollama provider wire helpers, also preserved at the crate root.
pub mod ollama {
    pub use crate::ollama::{
        OllamaCodec, OllamaCodecLib, OllamaRequestOptions, decode_ollama_response,
        decode_ollama_stream, encode_ollama_request,
    };
}
