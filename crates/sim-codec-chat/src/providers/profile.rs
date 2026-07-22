//! Open provider-codec profile records.
//!
//! These values are ordinary data owned by the codec crate. Provider identity
//! stays outside the kernel so new providers can be added without expanding a
//! closed enum in `sim-kernel`.

use sim_kernel::Symbol;

/// Provider request-body family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestWire {
    /// OpenAI Responses API request shape.
    OpenAiResponses,
    /// OpenAI chat-completions request shape.
    OpenAiChat,
    /// Anthropic Messages request shape.
    AnthropicMessages,
    /// Ollama `/api/chat` request shape.
    OllamaChat,
}

/// Provider streaming frame family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamWire {
    /// Provider has no streaming frame shape in this profile.
    None,
    /// Server-sent-event frames.
    Sse,
    /// Newline-delimited JSON frames.
    Ndjson,
}

/// Data profile for a provider codec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecProfile {
    /// Runtime codec symbol such as `codec:openai`.
    pub codec: Symbol,
    /// Open provider id such as `openai` or `ollama`.
    pub provider: Symbol,
    /// Request wire shape accepted by the provider.
    pub request_wire: RequestWire,
    /// Stream frame shape emitted by the provider.
    pub stream_wire: StreamWire,
}

impl CodecProfile {
    /// Builds a provider profile from its open data fields.
    pub fn new(
        codec: Symbol,
        provider: Symbol,
        request_wire: RequestWire,
        stream_wire: StreamWire,
    ) -> Self {
        Self {
            codec,
            provider,
            request_wire,
            stream_wire,
        }
    }
}

/// Profile for the native OpenAI provider codec.
pub fn openai_profile() -> CodecProfile {
    CodecProfile::new(
        Symbol::qualified("codec", "openai"),
        Symbol::new("openai"),
        RequestWire::OpenAiChat,
        StreamWire::Sse,
    )
}

/// Profile for the native Anthropic provider codec.
pub fn anthropic_profile() -> CodecProfile {
    CodecProfile::new(
        Symbol::qualified("codec", "anthropic"),
        Symbol::new("anthropic"),
        RequestWire::AnthropicMessages,
        StreamWire::Sse,
    )
}

/// Profile for the native Ollama provider codec.
pub fn ollama_profile() -> CodecProfile {
    CodecProfile::new(
        Symbol::qualified("codec", "ollama"),
        Symbol::new("ollama"),
        RequestWire::OllamaChat,
        StreamWire::Ndjson,
    )
}

/// Profile for the native LM Studio provider codec.
pub fn lm_studio_profile() -> CodecProfile {
    CodecProfile::new(
        Symbol::qualified("codec", "lm-studio"),
        Symbol::new("lm-studio"),
        RequestWire::OpenAiChat,
        StreamWire::Sse,
    )
}

/// Profile for the native Lemonade provider codec.
pub fn lemonade_profile() -> CodecProfile {
    CodecProfile::new(
        Symbol::qualified("codec", "lemonade"),
        Symbol::new("lemonade"),
        RequestWire::OpenAiChat,
        StreamWire::Sse,
    )
}
