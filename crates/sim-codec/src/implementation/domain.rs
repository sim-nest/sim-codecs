//! Builder for domain codec libs.
//!
//! A "domain codec" lib (`codec:chat`, `codec:scene`, `codec:intent`, ...) was
//! ~120 lines of near-identical scaffold: a UTF-8 `input_text` helper, a
//! `Decoder`+`Encoder`, a `LibManifest` with one `Export::Codec`, and a
//! `CodecRuntime` with ~10 fields mostly `None`. This collapses that to a few
//! lines of glue: implement `Decoder`+`Encoder`, then build a [`DomainCodecLib`].

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, CodecId, DefaultFactory, Dependency, Export, Factory, Lib, LibManifest, LibTarget,
    Linker, LoadCx, Result, ShapeRef, Symbol, Version,
};

use crate::{CodecDefaultDecode, CodecRuntime, Decoder, Encoder, Input, codec_value};

/// Read a codec `Input` as UTF-8 text, tagging any error with `codec`. The
/// shared body of every domain codec's `input_text` helper.
pub fn domain_input_text(codec: CodecId, input: Input) -> Result<String> {
    input.into_string_for(codec)
}

/// Resolve a general codec's expression shape: the `primary` symbol, then
/// `core/Expr`, then `core/Any`, then nil. This is the shared fallback chain
/// that every general-purpose codec (json/binary/binary-base64/algol/lisp/doc)
/// hand-rolled identically before OVERLAP6.07.
pub fn resolve_expr_shape(linker: &Linker, primary: &Symbol) -> Result<ShapeRef> {
    let nil = DefaultFactory.nil()?;
    Ok(linker
        .registry()
        .shape_by_symbol(primary)
        .or_else(|| {
            linker
                .registry()
                .shape_by_symbol(&Symbol::qualified("core", "Expr"))
        })
        .or_else(|| {
            linker
                .registry()
                .shape_by_symbol(&Symbol::qualified("core", "Any"))
        })
        .cloned()
        .unwrap_or(nil))
}

/// Resolve a general codec's encode-options shape: `core/EncodeOptions`, then
/// `core/Any`, then nil. The shared fallback chain those codecs hand-rolled.
pub fn resolve_options_shape(linker: &Linker) -> Result<ShapeRef> {
    let nil = DefaultFactory.nil()?;
    Ok(linker
        .registry()
        .shape_by_symbol(&Symbol::qualified("core", "EncodeOptions"))
        .or_else(|| {
            linker
                .registry()
                .shape_by_symbol(&Symbol::qualified("core", "Any"))
        })
        .cloned()
        .unwrap_or(nil))
}

/// A host-registered lib that exports one codec (and optionally the Shapes it
/// uses) built from a decoder and encoder.
pub struct DomainCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
    decoder: Arc<dyn Decoder>,
    encoder: Arc<dyn Encoder>,
    expr_shape_symbol: Symbol,
    shapes: Vec<(Symbol, ShapeRef)>,
}

impl DomainCodecLib {
    /// Build a domain codec lib. `expr_shape_symbol` is the codec's expression
    /// shape; it is resolved (at load) from the lib's own registered shapes,
    /// then the registry, then `core/Expr`, then `core/Any`, then nil.
    pub fn new(
        symbol: Symbol,
        codec_id: CodecId,
        decoder: Arc<dyn Decoder>,
        encoder: Arc<dyn Encoder>,
        expr_shape_symbol: Symbol,
    ) -> Self {
        Self {
            symbol,
            codec_id,
            decoder,
            encoder,
            expr_shape_symbol,
            shapes: Vec::new(),
        }
    }

    /// Also register these Shapes when the lib loads (for codecs like
    /// `codec:scene` that own their domain's node Shapes).
    pub fn with_shapes(mut self, shapes: Vec<(Symbol, ShapeRef)>) -> Self {
        self.shapes = shapes;
        self
    }
}

impl Lib for DomainCodecLib {
    fn manifest(&self) -> LibManifest {
        let mut exports: Vec<Export> = self
            .shapes
            .iter()
            .map(|(symbol, _)| Export::Shape {
                symbol: symbol.clone(),
                shape_id: None,
            })
            .collect();
        exports.push(Export::Codec {
            symbol: self.symbol.clone(),
            codec_id: Some(self.codec_id),
        });
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

    fn load(&self, _cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        for (symbol, shape) in &self.shapes {
            linker.shape_value(symbol.clone(), shape.clone())?;
        }
        let nil = DefaultFactory.nil()?;
        let expr_shape = self
            .shapes
            .iter()
            .find(|(symbol, _)| symbol == &self.expr_shape_symbol)
            .map(|(_, shape)| shape.clone())
            .or_else(|| {
                linker
                    .registry()
                    .shape_by_symbol(&self.expr_shape_symbol)
                    .cloned()
            })
            .or_else(|| {
                linker
                    .registry()
                    .shape_by_symbol(&Symbol::qualified("core", "Expr"))
                    .cloned()
            })
            .or_else(|| {
                linker
                    .registry()
                    .shape_by_symbol(&Symbol::qualified("core", "Any"))
                    .cloned()
            })
            .unwrap_or_else(|| nil.clone());
        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(self.decoder.clone()),
                located_decoder: None,
                tree_decoder: None,
                encoder: Some(self.encoder.clone()),
                located_encoder: None,
                tree_encoder: None,
                expr_shape,
                options_shape: nil,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        Ok(())
    }
}
