//! Anthropic Messages provider wire codec.
//!
//! The free functions project Anthropic Messages JSON to and from the
//! canonical chat transcript maps. `AnthropicCodecLib` installs the same
//! mapping as a runtime codec under `codec:anthropic`.

mod common;
mod decode;
mod encode;
mod runtime;

pub use decode::{
    decode_anthropic_request, decode_anthropic_response, decode_anthropic_stream,
    decode_anthropic_stream_events,
};
pub use encode::{encode_anthropic_request, encode_anthropic_response};
pub use runtime::{AnthropicCodec, AnthropicCodecLib};

use sim_kernel::{CodecId, Symbol};

pub(super) const ANTHROPIC_CODEC_ID: CodecId = CodecId(0);

/// Options for Anthropic Messages request JSON generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnthropicCodecOptions {
    /// Model identifier to place in the generated request.
    pub model: String,
    /// Maximum output tokens for the request body.
    pub max_tokens: u64,
    /// Whether to request a streamed SSE response.
    pub stream: bool,
    /// Whether to include tool schemas from the request transcript.
    pub tools: bool,
}

impl AnthropicCodecOptions {
    /// Builds codec options from a model id, token budget, and stream/tools flags.
    pub fn new(model: impl Into<String>, max_tokens: u64, stream: bool, tools: bool) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            stream,
            tools,
        }
    }
}

/// Alias for [`AnthropicCodecOptions`] when used for provider request generation.
pub type AnthropicRequestOptions = AnthropicCodecOptions;

/// Returns the codec symbol `codec:anthropic`.
pub fn anthropic_codec_symbol() -> Symbol {
    Symbol::qualified("codec", "anthropic")
}
