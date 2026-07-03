//! The `codec:doc` decoder/encoder and its host-registered lib. Decodes
//! document text into a document `Expr` and encodes document values or strings
//! back to text, installing the `doc/chunk-*` chunking functions alongside.

use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, DecodeBudget, Decoder, Encoder, Input, Output, ReadCx,
    codec_value,
};
use sim_kernel::{
    AbiVersion, Cx, DefaultFactory, Dependency, Error, Export, Lib, LibManifest, LibTarget, Linker,
    Result, Symbol, Version, WriteCx,
};

use crate::document::{DocValue, decode_document};
use crate::functions::{CHUNK_FUNCTIONS, DocChunkFunction};

/// The `codec:doc` decoder/encoder.
///
/// As a [`Decoder`] it turns document text into a document `Expr`; as an
/// [`Encoder`] it writes a document value or a raw string back to text and
/// fails closed on any other expression.
pub struct DocCodec;

impl Decoder for DocCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr> {
        let source = input.into_string()?;
        let budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        Ok(decode_document(&source).as_expr())
    }
}

impl Encoder for DocCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &sim_kernel::Expr) -> Result<Output> {
        match expr {
            sim_kernel::Expr::String(text) => Ok(Output::Text(text.clone())),
            sim_kernel::Expr::Map(_) => {
                let doc = DocValue::from_expr(expr).map_err(|err| Error::CodecError {
                    codec: cx.codec,
                    message: err.to_string(),
                })?;
                Ok(Output::Text(doc.text))
            }
            _ => Err(Error::CodecError {
                codec: cx.codec,
                message: "codec:doc encodes document values or strings".to_owned(),
            }),
        }
    }
}

/// The host-registered [`Lib`] that installs [`DocCodec`] as `codec:doc` and
/// the `doc/chunk-*` chunking functions.
pub struct DocCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}

impl DocCodecLib {
    /// Create the lib bound to the given codec id (obtained from
    /// [`Registry::fresh_codec_id`](sim_kernel::Registry::fresh_codec_id)).
    pub fn new(id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "doc"),
            codec_id: id,
        }
    }
}

impl Lib for DocCodecLib {
    fn manifest(&self) -> LibManifest {
        let mut exports = vec![Export::Codec {
            symbol: self.symbol.clone(),
            codec_id: Some(self.codec_id),
        }];
        exports.extend(CHUNK_FUNCTIONS.iter().map(|kind| Export::Function {
            symbol: kind.symbol(),
            function_id: None,
        }));
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports,
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let _factory = DefaultFactory;
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "Document"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(DocCodec)),
                located_decoder: None,
                tree_decoder: None,
                encoder: Some(Arc::new(DocCodec)),
                located_encoder: None,
                tree_encoder: None,
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;

        for kind in CHUNK_FUNCTIONS {
            linker.function_value(
                kind.symbol(),
                cx.factory()
                    .opaque(Arc::new(DocChunkFunction::new(*kind)))?,
            )?;
        }
        Ok(())
    }
}

/// Install [`DocCodecLib`] into `cx`, registering `codec:doc` and the
/// `doc/chunk-*` functions with a freshly allocated codec id.
pub fn install_doc_codec(cx: &mut Cx) -> Result<()> {
    let lib = DocCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib)?;
    Ok(())
}
