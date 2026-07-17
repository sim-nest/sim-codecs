//! Shared OpenAI-compatible provider helpers.

use sim_codec::{DecodeLimits, Input};
use sim_kernel::{CodecId, Expr, Result, Symbol};

use super::openai::{
    OpenAiRequestOptions, decode_openai_request_for_codec_with_limits,
    decode_openai_response_for_codec_with_limits, decode_openai_stream_for_codec_with_limits,
    encode_openai_request, encode_openai_response, encode_openai_response_for_codec,
};

/// Decodes an OpenAI-shaped request and stamps the provider identity.
pub(in crate::providers) fn decode_request_for_provider(
    codec: CodecId,
    input: Input,
    provider: &'static str,
    limits: DecodeLimits,
) -> Result<Expr> {
    let mut expr = decode_openai_request_for_codec_with_limits(codec, input, limits)?;
    stamp_provider(&mut expr, provider);
    Ok(expr)
}

/// Encodes a request through the OpenAI chat-completions wire shape.
pub(in crate::providers) fn encode_request_for_provider(
    expr: &Expr,
    options: &OpenAiRequestOptions,
) -> Result<Vec<u8>> {
    encode_openai_request(expr, options)
}

/// Decodes an OpenAI-shaped response and stamps the provider identity.
pub(in crate::providers) fn decode_response_for_provider(
    codec: CodecId,
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
    provider: &'static str,
    limits: DecodeLimits,
) -> Result<Expr> {
    let mut expr = decode_openai_response_for_codec_with_limits(
        codec,
        runner,
        model,
        body,
        include_raw,
        limits,
    )?;
    stamp_provider(&mut expr, provider);
    Ok(expr)
}

/// Decodes OpenAI-shaped SSE chunks and stamps the provider identity.
pub(in crate::providers) fn decode_stream_for_provider(
    codec: CodecId,
    runner: Symbol,
    model: &str,
    body: &[u8],
    include_raw: bool,
    provider: &'static str,
    limits: DecodeLimits,
) -> Result<Expr> {
    let mut expr = decode_openai_stream_for_codec_with_limits(
        codec,
        runner,
        model,
        body,
        include_raw,
        limits,
    )?;
    stamp_provider(&mut expr, provider);
    Ok(expr)
}

/// Encodes a response through the runtime OpenAI chat-completions wire shape.
pub(in crate::providers) fn encode_response_for_codec(
    codec: CodecId,
    expr: &Expr,
) -> Result<String> {
    encode_openai_response_for_codec(codec, expr)
}

/// Encodes a response through the public OpenAI chat-completions wire shape.
pub(in crate::providers) fn encode_response_for_provider(expr: &Expr) -> Result<Vec<u8>> {
    encode_openai_response(expr)
}

fn stamp_provider(expr: &mut Expr, provider: &'static str) {
    let Expr::Map(entries) = expr else {
        return;
    };
    let value = Expr::Symbol(Symbol::new(provider));
    if let Some((_, existing)) = entries.iter_mut().find(|(key, _)| provider_key(key)) {
        *existing = value;
    } else {
        entries.push((Expr::Symbol(Symbol::new("provider")), value));
    }
}

fn provider_key(expr: &Expr) -> bool {
    matches!(expr, Expr::Symbol(symbol) if symbol.name.as_ref() == "provider")
}
