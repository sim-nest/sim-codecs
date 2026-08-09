//! Loadable `codec/javascript` runtime object.

use crate::{
    JAVASCRIPT_CODEC_ID, decode_javascript, decode_javascript_located, decode_javascript_tree,
    encode_javascript,
};
use sim_codec::{
    CodecDefaultDecode, CodecRuntime, DecodeBudget, Decoder, Encoder, Input, LocatedDecoder,
    Output, ReadCx, TreeDecoder, TreeEncoder, codec_value, validate_expr_tree,
};
use sim_kernel::{
    AbiVersion, Dependency, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker, LocatedExpr,
    LocatedExprTree, Result, Symbol, Version, WriteCx,
};
use std::sync::Arc;

/// Runtime codec object for `codec/javascript`.
#[derive(Default)]
pub struct JavascriptCodec;

impl Decoder for JavascriptCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input_text(cx.codec, input)?;
        let mut budget = DecodeBudget::new(cx.limits);
        decode_javascript(cx, &source, &mut budget)
    }
}
impl LocatedDecoder for JavascriptCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr> {
        decode_javascript_located(cx, source_id, input)
    }
}
impl TreeDecoder for JavascriptCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        decode_javascript_tree(cx, source_id, input)
    }
}
impl Encoder for JavascriptCodec {
    fn encode(&self, _cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_javascript(expr)
    }
}
impl TreeEncoder for JavascriptCodec {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, tree: &LocatedExprTree) -> Result<Output> {
        validate_expr_tree(cx.codec, tree)?;
        if cx.options.lossless_origin
            && let Some(origin) = &tree.origin
            && let Some(bytes) = cx.cx.sources().slice(origin)
        {
            return Ok(Output::Text(
                std::str::from_utf8(bytes)
                    .map_err(|error| Error::CodecError {
                        codec: cx.codec,
                        message: format!("javascript source origin is not UTF-8: {error}"),
                    })?
                    .to_owned(),
            ));
        }
        encode_javascript(&tree.expr)
    }
}

/// Host-registered library installing `codec/javascript`.
pub struct JavascriptCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}
impl JavascriptCodecLib {
    /// Construct with a host-assigned codec id.
    #[must_use]
    pub fn new(codec_id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "javascript"),
            codec_id,
        }
    }
}
impl Default for JavascriptCodecLib {
    fn default() -> Self {
        Self::new(JAVASCRIPT_CODEC_ID)
    }
}
impl Lib for JavascriptCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol: self.symbol.clone(),
                codec_id: Some(self.codec_id),
            }],
        }
    }
    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker) -> Result<()> {
        let expr_shape = sim_codec::resolve_expr_shape(
            linker,
            &Symbol::qualified("codec", "JavaScriptSurface"),
        )?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;
        let codec = Arc::new(JavascriptCodec);
        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(codec.clone()),
                located_decoder: Some(codec.clone()),
                tree_decoder: Some(codec.clone()),
                encoder: Some(codec.clone()),
                located_encoder: None,
                tree_encoder: Some(codec),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::TermInEvalDatumOtherwise,
            }),
        )?;
        Ok(())
    }
}

fn input_text(codec: sim_kernel::CodecId, input: Input) -> Result<String> {
    match input {
        Input::Text(text) => Ok(text),
        Input::Bytes(bytes) => String::from_utf8(bytes).map_err(|error| Error::CodecError {
            codec,
            message: format!("codec input is not valid UTF-8: {error}"),
        }),
    }
}
