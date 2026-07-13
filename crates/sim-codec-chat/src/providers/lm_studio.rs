//! LM Studio OpenAI-compatible provider wire codec.
//!
//! LM Studio accepts OpenAI chat-completions request and response bodies. This
//! module delegates the wire translation to the OpenAI codec while preserving
//! the native `lm-studio` provider identity on decoded transcripts.

use std::sync::Arc;

use sim_codec::{Decoder, DomainCodecLib, Encoder, Input, Output, ReadCx};
use sim_kernel::{CodecId, Expr, Lib, LibManifest, Linker, LoadCx, Result, Symbol, WriteCx};

use super::openai::OpenAiCodecOptions;
use super::openai_compat::{
    decode_request_for_provider, decode_response_for_provider, decode_stream_for_provider,
    encode_request_for_provider, encode_response_for_codec, encode_response_for_provider,
};

const LM_STUDIO_CODEC_ID: CodecId = CodecId(0);
const PROVIDER: &str = "lm-studio";

/// Options for LM Studio OpenAI-compatible request JSON generation.
pub type LmStudioCodecOptions = OpenAiCodecOptions;

/// Alias for [`LmStudioCodecOptions`] when used for request generation.
pub type LmStudioRequestOptions = LmStudioCodecOptions;

/// Runtime codec for LM Studio OpenAI-compatible chat-completion JSON.
pub struct LmStudioCodec;

impl Decoder for LmStudioCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        decode_request_for_provider(cx.codec, input, PROVIDER)
    }
}

impl Encoder for LmStudioCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_response_for_codec(cx.codec, expr).map(Output::Text)
    }
}

/// Host-registered lib for `codec:lm-studio`.
pub struct LmStudioCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl LmStudioCodecLib {
    /// Creates the lib bound to the given runtime-assigned codec id.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: lm_studio_codec_symbol(),
            codec_id: id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(LmStudioCodec),
            Arc::new(LmStudioCodec),
            Symbol::qualified("codec", "LmStudioTranscript"),
        )
    }
}

impl Lib for LmStudioCodecLib {
    fn manifest(&self) -> LibManifest {
        self.domain_lib().manifest()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        self.domain_lib().load(cx, linker)
    }
}

/// Returns the codec symbol `codec:lm-studio`.
pub fn lm_studio_codec_symbol() -> Symbol {
    Symbol::qualified("codec", PROVIDER)
}

/// Decodes an LM Studio OpenAI-compatible request body.
pub fn decode_lm_studio_request(input: Input) -> Result<Expr> {
    decode_request_for_provider(LM_STUDIO_CODEC_ID, input, PROVIDER)
}

/// Encodes a model-request transcript into an LM Studio request body.
pub fn encode_lm_studio_request(expr: &Expr, options: &LmStudioRequestOptions) -> Result<Vec<u8>> {
    encode_request_for_provider(expr, options)
}

/// Decodes an LM Studio OpenAI-compatible response body.
pub fn decode_lm_studio_response(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    decode_response_for_provider(runner, model, body, include_raw, PROVIDER)
}

/// Decodes LM Studio OpenAI-compatible SSE chunks into a response transcript.
pub fn decode_lm_studio_stream(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    decode_stream_for_provider(runner, model, body, include_raw, PROVIDER)
}

/// Encodes a model-response transcript into an LM Studio response body.
pub fn encode_lm_studio_response(expr: &Expr) -> Result<Vec<u8>> {
    encode_response_for_provider(expr)
}
