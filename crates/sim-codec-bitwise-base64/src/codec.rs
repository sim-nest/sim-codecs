//! The `BitwiseBase64Codec` runtime object and its `Lib` registration.
//!
//! Implements the codec traits by delegating frame encode/decode to
//! `sim-codec-bitwise` and adding the base64 text wrapping on either side. The
//! base64 layer is shared from `sim-codec-binary-base64` rather than forked.

use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, Decoder, Encoder, Input, LocatedDecoder, LocatedEncoder,
    Output, ReadCx, TreeDecoder, TreeEncoder, codec_value, validate_expr_tree,
};
use sim_codec_binary_base64::{decode_base64_with_limits, encode_base64};
use sim_kernel::{
    AbiVersion, CodecId, DefaultFactory, Dependency, Error, Export, Expr, Lib, LibManifest,
    LibTarget, Linker, LocatedExpr, LocatedExprTree, Result, Symbol, Version, WriteCx,
};

use crate::cookbook::{BitwiseBase64RoundtripReport, roundtrip_report_symbol};

/// Codec runtime object that carries `sim-codec-bitwise` frames as base64 text.
///
/// This domain codec is a thin text wrapper: it implements every codec role --
/// [`Decoder`]/[`Encoder`], located [`LocatedDecoder`]/[`LocatedEncoder`], and
/// tree [`TreeDecoder`]/[`TreeEncoder`] -- by delegating frame encode/decode to
/// `sim-codec-bitwise` and base64-encoding the bytes on the way out and
/// base64-decoding them on the way in. The base64 text and the underlying bytes
/// are untrusted data; malformed input fails closed and is never executed.
pub struct BitwiseBase64Codec;

impl Decoder for BitwiseBase64Codec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        decode_tree(cx, input).map(|tree| tree.located().expr)
    }
}

impl Encoder for BitwiseBase64Codec {
    fn encode(&self, _cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        let frame = sim_codec_bitwise::encode_frame(expr)?;
        Ok(Output::Text(encode_base64(&frame.0)))
    }
}

impl LocatedDecoder for BitwiseBase64Codec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExpr> {
        decode_tree(cx, input).map(|tree| tree.located())
    }
}

impl LocatedEncoder for BitwiseBase64Codec {
    fn encode_located(&self, cx: &mut WriteCx<'_>, expr: &LocatedExpr) -> Result<Output> {
        let frame = sim_codec_bitwise::encode_located_frame(expr, cx.options.lossless_origin)?;
        Ok(Output::Text(encode_base64(&frame.0)))
    }
}

impl TreeDecoder for BitwiseBase64Codec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExprTree> {
        decode_tree(cx, input)
    }
}

impl TreeEncoder for BitwiseBase64Codec {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output> {
        validate_expr_tree(cx.codec, expr)?;
        let frame = sim_codec_bitwise::encode_located_tree_frame(expr, cx.options.lossless_origin)?;
        Ok(Output::Text(encode_base64(&frame.0)))
    }
}

fn decode_tree(cx: &mut ReadCx<'_>, input: Input) -> Result<LocatedExprTree> {
    let text = input_text(cx.codec, input)?;
    let bytes = decode_base64_with_limits(cx.codec, &text, cx.limits)?;
    sim_codec_bitwise::decode_located_tree_frame_with_limits(
        cx.codec,
        &bytes,
        sim_codec_bitwise::DecodeLimits::from(cx.limits),
    )
    .map(|(_, tree)| tree)
}

fn input_text(codec: CodecId, input: Input) -> Result<String> {
    match input {
        Input::Text(text) => Ok(text),
        Input::Bytes(bytes) => String::from_utf8(bytes).map_err(|err| Error::CodecError {
            codec,
            message: format!("bitwise-base64 input is not valid UTF-8: {err}"),
        }),
    }
}

/// [`Lib`] that registers the bitwise-base64 codec with the runtime.
///
/// Its manifest exports the `codec/bitwise-base64` codec, and loading wires a
/// [`BitwiseBase64Codec`] into the linker as the decode and encode surface for
/// all codec roles.
pub struct BitwiseBase64CodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl BitwiseBase64CodecLib {
    /// Creates the codec lib bound to the runtime-assigned `id` for
    /// `codec/bitwise-base64`.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "bitwise-base64"),
            codec_id: id,
        }
    }
}

impl Lib for BitwiseBase64CodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: vec![
                Export::Codec {
                    symbol: self.symbol.clone(),
                    codec_id: Some(self.codec_id),
                },
                Export::Function {
                    symbol: roundtrip_report_symbol(),
                    function_id: None,
                },
            ],
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker) -> Result<()> {
        let _factory = DefaultFactory;
        let expr_shape = sim_codec::resolve_expr_shape(
            linker,
            &Symbol::qualified("codec", "BitwiseBase64Text"),
        )?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(BitwiseBase64Codec)),
                located_decoder: Some(Arc::new(BitwiseBase64Codec)),
                tree_decoder: Some(Arc::new(BitwiseBase64Codec)),
                encoder: Some(Arc::new(BitwiseBase64Codec)),
                located_encoder: Some(Arc::new(BitwiseBase64Codec)),
                tree_encoder: Some(Arc::new(BitwiseBase64Codec)),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        linker.function_value(
            roundtrip_report_symbol(),
            cx.factory()
                .opaque(Arc::new(BitwiseBase64RoundtripReport))?,
        )?;
        Ok(())
    }
}
