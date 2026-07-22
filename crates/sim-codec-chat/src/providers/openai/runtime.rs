use std::sync::Arc;

use sim_codec::{Decoder, DomainCodecLib, Encoder, Input, Output, ReadCx};
use sim_kernel::{CodecId, Expr, Lib, LibManifest, Linker, LoadCx, Result, Symbol, WriteCx};

use super::{
    decode::decode_openai_request_for_codec_with_limits, encode::encode_openai_response_for_codec,
    openai_codec_symbol,
};

/// Runtime codec for OpenAI-compatible chat-completion JSON.
pub struct OpenAiCodec;

impl Decoder for OpenAiCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        decode_openai_request_for_codec_with_limits(cx.codec, input, cx.limits)
    }
}

impl Encoder for OpenAiCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_openai_response_for_codec(cx.codec, expr).map(Output::Text)
    }
}

/// Host-registered lib for `codec:openai`.
pub struct OpenAiCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl OpenAiCodecLib {
    /// Creates the lib bound to the given runtime-assigned codec id.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: openai_codec_symbol(),
            codec_id: id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(OpenAiCodec),
            Arc::new(OpenAiCodec),
            Symbol::qualified("codec", "OpenAiTranscript"),
        )
    }
}

impl Lib for OpenAiCodecLib {
    fn manifest(&self) -> LibManifest {
        self.domain_lib().manifest()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        self.domain_lib().load(cx, linker)
    }
}
