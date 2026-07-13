//! The markup and `codec:doc` decoder/encoder host libs.
//!
//! Markup backends install as strict `codec:markup/<id>` runtime codecs.
//! `codec:doc` remains as the compatibility document codec and chunk-function
//! host lib.

use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, DecodeBudget, Decoder, Encoder, Input, Output, ReadCx,
    codec_value,
};
use sim_kernel::{
    AbiVersion, Cx, DefaultFactory, Dependency, Error, Export, Lib, LibManifest, LibTarget, Linker,
    Result, Symbol, Version, WriteCx,
};

use crate::backend::{
    BackendRegistry, MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions, MarkupError,
    default_backend_registry,
};
use crate::document::DocValue;
use crate::functions::{CHUNK_FUNCTIONS, DocChunkFunction};
use crate::markdown::MarkdownBackend;
use crate::markup::MarkupDoc;

/// Textual prefix for rendered markup codec symbols.
pub const MARKUP_CODEC_PREFIX: &str = "codec/markup/";

/// Return the runtime codec symbol for a markup backend id.
pub fn markup_codec_symbol(id: &crate::BackendId) -> Symbol {
    Symbol::qualified("codec", format!("markup/{}", id.as_str()))
}

/// Strict runtime codec for one markup backend.
pub struct MarkupCodec {
    backend: Arc<dyn MarkupBackend>,
}

impl MarkupCodec {
    /// Create a codec backed by `backend`.
    pub fn new(backend: Arc<dyn MarkupBackend>) -> Self {
        Self { backend }
    }

    /// Return this codec's backend id.
    pub fn backend_id(&self) -> crate::BackendId {
        self.backend.id()
    }
}

impl Decoder for MarkupCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr> {
        let source = input.into_string()?;
        let budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let (doc, _fidelity) = self
            .backend
            .decode(&source, &MarkupDecodeOptions::default())
            .map_err(|err| err.into_kernel_error(cx.codec))?;
        Ok(doc.as_expr())
    }
}

impl Encoder for MarkupCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &sim_kernel::Expr) -> Result<Output> {
        let doc = MarkupDoc::from_expr(expr).map_err(|err| {
            MarkupError::InvalidDocument(err.to_string()).into_kernel_error(cx.codec)
        })?;
        let (source, fidelity) = self
            .backend
            .encode(&doc, &MarkupEncodeOptions::default())
            .map_err(|err| err.into_kernel_error(cx.codec))?;
        if !fidelity.dropped.is_empty() {
            return Err(MarkupError::Encode(format!(
                "backend {} reported {} dropped part(s)",
                fidelity.backend,
                fidelity.dropped.len()
            ))
            .into_kernel_error(cx.codec));
        }
        Ok(Output::Text(source))
    }
}

/// The `codec:doc` decoder/encoder.
///
/// As a [`Decoder`] it turns document text into a markup document `Expr`; as an
/// [`Encoder`] it writes a markup document, compatibility document value, or raw
/// string back to text and fails closed on any other expression.
pub struct DocCodec;

impl Decoder for DocCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr> {
        let source = input.into_string()?;
        let budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let (doc, _fidelity) = MarkdownBackend
            .decode(&source, &MarkupDecodeOptions::default())
            .map_err(|err| err.into_kernel_error(cx.codec))?;
        Ok(doc.as_expr())
    }
}

impl Encoder for DocCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &sim_kernel::Expr) -> Result<Output> {
        match expr {
            sim_kernel::Expr::String(text) => Ok(Output::Text(text.clone())),
            sim_kernel::Expr::Map(_) => Ok(Output::Text(encode_doc_expr(expr).map_err(|err| {
                Error::CodecError {
                    codec: cx.codec,
                    message: err.to_string(),
                }
            })?)),
            _ => Err(Error::CodecError {
                codec: cx.codec,
                message: "codec:doc encodes document values or strings".to_owned(),
            }),
        }
    }
}

fn encode_doc_expr(expr: &sim_kernel::Expr) -> Result<String> {
    match MarkupDoc::from_expr(expr) {
        Ok(doc) => Ok(doc.to_source_text()),
        Err(markup_error) => match DocValue::from_expr(expr) {
            Ok(doc) => Ok(doc.text),
            Err(_) => Err(markup_error),
        },
    }
}

struct RegisteredBackend {
    backend: Arc<dyn MarkupBackend>,
    codec_id: sim_kernel::CodecId,
}

struct MarkupCodecLib {
    backends: Vec<RegisteredBackend>,
}

impl MarkupCodecLib {
    fn new(cx: &mut Cx, registry: BackendRegistry) -> Self {
        let backends = registry
            .iter()
            .map(|(_, backend)| RegisteredBackend {
                backend: backend.clone(),
                codec_id: cx.registry_mut().fresh_codec_id(),
            })
            .collect();
        Self { backends }
    }
}

impl Lib for MarkupCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("codec", "markup"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: self
                .backends
                .iter()
                .map(|registered| Export::Codec {
                    symbol: markup_codec_symbol(&registered.backend.id()),
                    codec_id: Some(registered.codec_id),
                })
                .collect(),
        }
    }

    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "Document"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        for registered in &self.backends {
            let symbol = markup_codec_symbol(&registered.backend.id());
            linker.codec_value(
                symbol.clone(),
                codec_value(CodecRuntime {
                    id: registered.codec_id,
                    symbol,
                    decoder: Some(Arc::new(MarkupCodec::new(registered.backend.clone()))),
                    located_decoder: None,
                    tree_decoder: None,
                    encoder: Some(Arc::new(MarkupCodec::new(registered.backend.clone()))),
                    located_encoder: None,
                    tree_encoder: None,
                    expr_shape: expr_shape.clone(),
                    options_shape: options_shape.clone(),
                    default_decode: CodecDefaultDecode::Datum,
                }),
            )?;
        }
        Ok(())
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
    install_markup_codecs(cx, default_backend_registry())?;
    let lib = DocCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib)?;
    Ok(())
}

/// Install each registered markup backend as `codec:markup/<id>`.
pub fn install_markup_codecs(cx: &mut Cx, registry: BackendRegistry) -> Result<()> {
    if registry.is_empty() {
        return Err(Error::Lib("markup backend registry is empty".to_owned()));
    }
    let lib = MarkupCodecLib::new(cx, registry);
    cx.load_lib(&lib)?;
    Ok(())
}
