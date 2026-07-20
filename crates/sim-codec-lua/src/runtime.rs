use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, DecodeBudget, Decoder, Encoder, Input, LocatedDecoder,
    Output, ReadCx, TreeDecoder, TreeEncoder, codec_value, validate_expr_tree,
};
use sim_kernel::{
    AbiVersion, Dependency, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker, LocatedExpr,
    LocatedExprTree, Result, Symbol, Version, WriteCx,
};

use crate::LUA_CODEC_ID;
use crate::encode::encode_lua_chunk_expr;
use crate::lower::{
    decode_lua_chunk, decode_lua_located_chunk, decode_lua_tree_chunk, input_text_for,
};

/// Runtime codec object for `codec/lua`.
#[derive(Default)]
pub struct LuaCodec;

impl Decoder for LuaCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input_text_for(cx.codec, input)?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        decode_lua_chunk(cx, "<lua>", &source, &mut budget)
    }
}

impl LocatedDecoder for LuaCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr> {
        decode_lua_located_chunk(cx, source_id, input)
    }
}

impl TreeDecoder for LuaCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        decode_lua_tree_chunk(cx, source_id, input)
    }
}

impl Encoder for LuaCodec {
    fn encode(&self, _cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_lua_chunk_expr(expr)
    }
}

impl TreeEncoder for LuaCodec {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, tree: &LocatedExprTree) -> Result<Output> {
        validate_expr_tree(cx.codec, tree)?;
        if cx.options.lossless_origin
            && let Some(origin) = &tree.origin
            && let Some(bytes) = cx.cx.sources().slice(origin)
        {
            let text = std::str::from_utf8(bytes).map_err(|err| Error::CodecError {
                codec: cx.codec,
                message: format!("lua source origin is not UTF-8: {err}"),
            })?;
            return Ok(Output::Text(text.to_owned()));
        }
        encode_lua_chunk_expr(&tree.expr)
    }
}

/// Host-registered library that installs `codec/lua`.
pub struct LuaCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}

impl LuaCodecLib {
    /// Creates a Lua codec lib with the supplied runtime codec id.
    pub fn new(codec_id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "lua"),
            codec_id,
        }
    }
}

impl Default for LuaCodecLib {
    fn default() -> Self {
        Self::new(LUA_CODEC_ID)
    }
}

impl Lib for LuaCodecLib {
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
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "LuaSurface"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;
        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(LuaCodec)),
                located_decoder: Some(Arc::new(LuaCodec)),
                tree_decoder: Some(Arc::new(LuaCodec)),
                encoder: Some(Arc::new(LuaCodec)),
                located_encoder: None,
                tree_encoder: Some(Arc::new(LuaCodec)),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::TermInEvalDatumOtherwise,
            }),
        )?;
        Ok(())
    }
}
