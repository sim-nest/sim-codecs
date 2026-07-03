//! The `JsonCodec` runtime object and its `Lib` registration.
//!
//! Wires the `Expr <-> JSON` conversions in this crate into the codec
//! decoder/encoder, located, and tree traits so JSON becomes a registered
//! codec surface in the runtime.

use std::sync::Arc;

use serde_json::Value as JsonValue;
use sim_codec::{
    CodecDefaultDecode, CodecRuntime, DecodeBudget, Decoder, Encoder, Input, LocatedDecoder,
    LocatedEncoder, Output, ReadCx, TreeDecoder, TreeEncoder, codec_value, validate_expr_tree,
};
use sim_kernel::{
    AbiVersion, DefaultFactory, Dependency, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LocatedExpr, LocatedExprTree, Result, Symbol, Version, WriteCx,
};

use crate::{
    expr_to_json, json_to_expr, json_to_located_expr, json_to_tree, located_expr_to_json,
    tree_to_json,
};

/// JSON codec runtime object that round-trips every [`Expr`] through JSON.
///
/// Implements all codec roles -- [`Decoder`]/[`Encoder`], the located
/// [`LocatedDecoder`]/[`LocatedEncoder`], and the tree
/// [`TreeDecoder`]/[`TreeEncoder`] -- by projecting the shared `Expr` graph onto
/// `$expr`-tagged (and `$located`) `serde_json::Value` forms, so any expression
/// the kernel can hold survives a JSON round-trip losslessly.
pub struct JsonCodec;

impl Decoder for JsonCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input.into_string()?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let value =
            serde_json::from_str::<JsonValue>(&source).map_err(|err| Error::CodecError {
                codec: cx.codec,
                message: err.to_string(),
            })?;
        json_to_expr(cx.codec, &value, &mut budget, 0)
    }
}

impl Encoder for JsonCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        let value = expr_to_json(expr);
        let text = serde_json::to_string(&value).map_err(|err| Error::CodecError {
            codec: cx.codec,
            message: err.to_string(),
        })?;
        Ok(Output::Text(text))
    }
}

impl LocatedDecoder for JsonCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExpr> {
        let source = input.into_string()?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let value =
            serde_json::from_str::<JsonValue>(&source).map_err(|err| Error::CodecError {
                codec: cx.codec,
                message: err.to_string(),
            })?;
        json_to_located_expr(cx.codec, &value, &mut budget, 0)
    }
}

impl LocatedEncoder for JsonCodec {
    fn encode_located(&self, cx: &mut WriteCx<'_>, expr: &LocatedExpr) -> Result<Output> {
        let value = located_expr_to_json(expr, cx.options.lossless_origin);
        let text = serde_json::to_string(&value).map_err(|err| Error::CodecError {
            codec: cx.codec,
            message: err.to_string(),
        })?;
        Ok(Output::Text(text))
    }
}

impl TreeDecoder for JsonCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExprTree> {
        let source = input.into_string()?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let value =
            serde_json::from_str::<JsonValue>(&source).map_err(|err| Error::CodecError {
                codec: cx.codec,
                message: err.to_string(),
            })?;
        json_to_tree(cx.codec, &value, &mut budget, 0)
    }
}

impl TreeEncoder for JsonCodec {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output> {
        validate_expr_tree(cx.codec, expr)?;
        let value = tree_to_json(expr, cx.options.lossless_origin);
        let text = serde_json::to_string(&value).map_err(|err| Error::CodecError {
            codec: cx.codec,
            message: err.to_string(),
        })?;
        Ok(Output::Text(text))
    }
}

/// [`Lib`] that registers the JSON codec with the runtime.
///
/// Its manifest exports the `codec/json` codec, and loading wires a [`JsonCodec`]
/// into the linker as the decode and encode surface for all codec roles.
pub struct JsonCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}

impl JsonCodecLib {
    /// Creates the codec lib bound to the runtime-assigned `id` for `codec/json`.
    pub fn new(id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "json"),
            codec_id: id,
        }
    }
}

impl Lib for JsonCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
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
        let _factory = DefaultFactory;
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "JsonTaggedExpr"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(JsonCodec)),
                located_decoder: Some(Arc::new(JsonCodec)),
                tree_decoder: Some(Arc::new(JsonCodec)),
                encoder: Some(Arc::new(JsonCodec)),
                located_encoder: Some(Arc::new(JsonCodec)),
                tree_encoder: Some(Arc::new(JsonCodec)),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        Ok(())
    }
}
