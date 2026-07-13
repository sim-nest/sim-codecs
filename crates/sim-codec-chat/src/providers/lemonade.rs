//! Lemonade OpenAI-compatible provider wire codec.
//!
//! Lemonade Server accepts OpenAI chat-completions request and response bodies.
//! This module delegates the wire translation to the OpenAI codec while
//! preserving the native `lemonade` provider identity on decoded transcripts.

use std::sync::Arc;

use sim_codec::{Decoder, DomainCodecLib, Encoder, Input, Output, ReadCx};
use sim_kernel::{CodecId, Expr, Lib, LibManifest, Linker, LoadCx, Result, Symbol, WriteCx};

use super::openai::OpenAiCodecOptions;
use super::openai_compat::{
    decode_request_for_provider, decode_response_for_provider, decode_stream_for_provider,
    encode_request_for_provider, encode_response_for_codec, encode_response_for_provider,
};

const LEMONADE_CODEC_ID: CodecId = CodecId(0);
const PROVIDER: &str = "lemonade";

/// Options for Lemonade OpenAI-compatible request JSON generation.
pub type LemonadeCodecOptions = OpenAiCodecOptions;

/// Alias for [`LemonadeCodecOptions`] when used for request generation.
pub type LemonadeRequestOptions = LemonadeCodecOptions;

/// Runtime codec for Lemonade OpenAI-compatible chat-completion JSON.
pub struct LemonadeCodec;

impl Decoder for LemonadeCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        decode_request_for_provider(cx.codec, input, PROVIDER)
    }
}

impl Encoder for LemonadeCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_response_for_codec(cx.codec, expr).map(Output::Text)
    }
}

/// Host-registered lib for `codec:lemonade`.
pub struct LemonadeCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl LemonadeCodecLib {
    /// Creates the lib bound to the given runtime-assigned codec id.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: lemonade_codec_symbol(),
            codec_id: id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(LemonadeCodec),
            Arc::new(LemonadeCodec),
            Symbol::qualified("codec", "LemonadeTranscript"),
        )
    }
}

impl Lib for LemonadeCodecLib {
    fn manifest(&self) -> LibManifest {
        self.domain_lib().manifest()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        self.domain_lib().load(cx, linker)
    }
}

/// Returns the codec symbol `codec:lemonade`.
pub fn lemonade_codec_symbol() -> Symbol {
    Symbol::qualified("codec", PROVIDER)
}

/// Decodes a Lemonade OpenAI-compatible request body.
pub fn decode_lemonade_request(input: Input) -> Result<Expr> {
    decode_request_for_provider(LEMONADE_CODEC_ID, input, PROVIDER)
}

/// Encodes a model-request transcript into a Lemonade request body.
pub fn encode_lemonade_request(expr: &Expr, options: &LemonadeRequestOptions) -> Result<Vec<u8>> {
    encode_request_for_provider(expr, options)
}

/// Decodes a Lemonade OpenAI-compatible response body.
pub fn decode_lemonade_response(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    decode_response_for_provider(runner, model, body, include_raw, PROVIDER)
}

/// Decodes Lemonade OpenAI-compatible SSE chunks into a response transcript.
pub fn decode_lemonade_stream(
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
) -> Result<Expr> {
    decode_stream_for_provider(runner, model, body, include_raw, PROVIDER)
}

/// Encodes a model-response transcript into a Lemonade response body.
pub fn encode_lemonade_response(expr: &Expr) -> Result<Vec<u8>> {
    encode_response_for_provider(expr)
}
