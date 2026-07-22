use std::sync::Arc;

use sim_codec::{Decoder, DomainCodecLib, Encoder, Input, Output, ReadCx};
use sim_kernel::{CodecId, Expr, Lib, LibManifest, Linker, LoadCx, Result, Symbol, WriteCx};

use super::{
    anthropic_codec_symbol, decode::decode_anthropic_request_for_codec_with_limits,
    encode::encode_anthropic_response_for_codec,
};

/// Runtime codec for Anthropic Messages JSON.
pub struct AnthropicCodec;

impl Decoder for AnthropicCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        decode_anthropic_request_for_codec_with_limits(cx.codec, input, cx.limits)
    }
}

impl Encoder for AnthropicCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_anthropic_response_for_codec(cx.codec, expr).map(Output::Text)
    }
}

/// Host-registered lib for `codec:anthropic`.
pub struct AnthropicCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl AnthropicCodecLib {
    /// Creates the lib bound to the given runtime-assigned codec id.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: anthropic_codec_symbol(),
            codec_id: id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(AnthropicCodec),
            Arc::new(AnthropicCodec),
            Symbol::qualified("codec", "AnthropicTranscript"),
        )
    }
}

impl Lib for AnthropicCodecLib {
    fn manifest(&self) -> LibManifest {
        self.domain_lib().manifest()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        self.domain_lib().load(cx, linker)
    }
}
