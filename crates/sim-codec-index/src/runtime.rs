//! Runtime registration for `codec/index`.

use std::sync::Arc;

use sim_codec::{CodecDefaultDecode, CodecRuntime, Decoder, Encoder, Input, Output, codec_value};
use sim_kernel::{
    AbiVersion, CodecId, Dependency, EncodePosition, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, Result, Symbol, Version,
};

use crate::{
    CodecError, IndexForm,
    error::kernel_codec_error,
    expr_from_index_doc,
    form::{doc_from_expr, doc_from_form, encode_doc},
};

/// Runtime decoder/encoder for the `codec/index` surface.
///
/// Plain decode accepts canonical s-expression text by default and also accepts
/// tagged JSON expression text when the input starts with `{`. Plain encode
/// emits the canonical s-expression form; callers that need JSON use
/// [`IndexCodec::encode`] directly with [`IndexForm::Json`].
pub struct IndexCodec;

impl IndexCodec {
    /// Decodes one index text form into a checked [`sim_index_core::IndexDoc`].
    pub fn decode(
        &self,
        form: IndexForm,
        source: &str,
    ) -> std::result::Result<sim_index_core::IndexDoc, CodecError> {
        doc_from_form(form, source)
    }

    /// Encodes a checked index document as s-expression or JSON text.
    pub fn encode(
        &self,
        doc: &sim_index_core::IndexDoc,
        position: EncodePosition,
        form: IndexForm,
    ) -> std::result::Result<String, CodecError> {
        encode_doc(doc, position, form)
    }
}

impl Decoder for IndexCodec {
    fn decode(&self, cx: &mut sim_codec::ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input.into_string_for(cx.codec)?;
        let form = detect_form(&source);
        let doc = doc_from_form(form, &source).map_err(|err| kernel_codec_error(cx.codec, err))?;
        Ok(expr_from_index_doc(&doc))
    }
}

impl Encoder for IndexCodec {
    fn encode(&self, cx: &mut sim_kernel::WriteCx<'_>, expr: &Expr) -> Result<Output> {
        let doc = doc_from_expr(expr).map_err(|err| kernel_codec_error(cx.codec, err))?;
        let text = encode_doc(&doc, cx.options.position, IndexForm::Sx)
            .map_err(|err| kernel_codec_error(cx.codec, err))?;
        Ok(Output::Text(text))
    }
}

/// Host-registered library that installs `codec/index`.
pub struct IndexCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl IndexCodecLib {
    /// Creates the codec lib bound to the runtime-assigned id for
    /// `codec/index`.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "index"),
            codec_id: id,
        }
    }
}

impl Lib for IndexCodecLib {
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

    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "Index"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;
        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(IndexCodec)),
                located_decoder: None,
                tree_decoder: None,
                encoder: Some(Arc::new(IndexCodec)),
                located_encoder: None,
                tree_encoder: None,
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        Ok(())
    }
}

fn detect_form(source: &str) -> IndexForm {
    if source.trim_start().starts_with('{') {
        IndexForm::Json
    } else {
        IndexForm::Sx
    }
}
