//! Loadable `codec/python` runtime object.

use crate::{
    PYTHON_CODEC_ID, decode_python, decode_python_located, decode_python_tree, encode_python,
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

/// Runtime codec object for `codec/python`.
#[derive(Default)]
pub struct PythonCodec;

impl Decoder for PythonCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input_text(cx.codec, input)?;
        let mut budget = DecodeBudget::new(cx.limits);
        decode_python(cx, &source, &mut budget)
    }
}
impl LocatedDecoder for PythonCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr> {
        decode_python_located(cx, source_id, input)
    }
}
impl TreeDecoder for PythonCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        decode_python_tree(cx, source_id, input)
    }
}
impl Encoder for PythonCodec {
    fn encode(&self, _cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_python(expr)
    }
}
impl TreeEncoder for PythonCodec {
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
                        message: format!("python source origin is not UTF-8: {error}"),
                    })?
                    .to_owned(),
            ));
        }
        encode_python(&tree.expr)
    }
}

/// Host-registered library installing `codec/python`.
pub struct PythonCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}
impl PythonCodecLib {
    /// Construct with a host-assigned codec id.
    pub fn new(codec_id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "python"),
            codec_id,
        }
    }
}
impl Default for PythonCodecLib {
    fn default() -> Self {
        Self::new(PYTHON_CODEC_ID)
    }
}
impl Lib for PythonCodecLib {
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
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "PythonSurface"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;
        let codec = Arc::new(PythonCodec);
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
