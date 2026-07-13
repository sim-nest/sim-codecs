//! OpenAI-compatible provider wire codec.
//!
//! The free functions project OpenAI chat-completion JSON to and from the
//! canonical chat transcript maps. `OpenAiCodecLib` installs the same mapping
//! as a runtime codec under `codec:openai`.

mod common;
mod decode;
mod encode;
mod runtime;

pub(in crate::providers) use decode::decode_openai_request_for_codec;
pub use decode::{decode_openai_request, decode_openai_response, decode_openai_stream};
pub(in crate::providers) use encode::encode_openai_response_for_codec;
pub use encode::{encode_openai_request, encode_openai_response};
pub use runtime::{OpenAiCodec, OpenAiCodecLib};

use sim_kernel::{CodecId, Symbol};

pub(super) const OPENAI_CODEC_ID: CodecId = CodecId(0);

/// Options for OpenAI-compatible provider request JSON generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenAiCodecOptions {
    /// Model identifier to place in the generated request.
    pub model: String,
    /// Whether to request a streamed SSE response.
    pub stream: bool,
    /// Whether to include the `tools` field in the generated request.
    pub tools: bool,
}

impl OpenAiCodecOptions {
    /// Builds codec options from a model id and the stream/tools flags.
    pub fn new(model: impl Into<String>, stream: bool, tools: bool) -> Self {
        Self {
            model: model.into(),
            stream,
            tools,
        }
    }
}

/// Alias for [`OpenAiCodecOptions`] when used for provider request generation.
pub type OpenAiRequestOptions = OpenAiCodecOptions;

/// Returns the codec symbol `codec:openai`.
pub fn openai_codec_symbol() -> Symbol {
    Symbol::qualified("codec", "openai")
}
