//! The core decoder/encoder runtime contracts.
//!
//! Defines `Input`/`Output`, the `DecodePosition`/`DecodeTarget` output-position
//! model, the `Decoder`/`Encoder` traits (with their located and tree variants),
//! and the `CodecRuntime` glue that registers a codec as a runtime
//! object.

use std::sync::Arc;

use sim_kernel::{
    ClassRef, CodecId, Cx, LocatedExpr, LocatedExprTree, Object, Result, ShapeRef, Symbol, Value,
    WriteCx,
};

use super::limits::ReadCx;

/// Raw input handed to a [`Decoder`]: source text or source bytes.
///
/// Text codecs take [`Input::Text`]; binary codecs take [`Input::Bytes`]. A text
/// codec given bytes interprets them as UTF-8 (see [`Input::into_string_for`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Source text.
    Text(String),
    /// Raw source bytes.
    Bytes(Vec<u8>),
}

impl Input {
    /// Take the input as UTF-8 text, decoding [`Input::Bytes`] and failing closed
    /// with a codec error if the bytes are not valid UTF-8.
    pub fn into_string(self) -> Result<String> {
        self.into_string_for(CodecId(0))
    }

    /// Take the input as UTF-8 text, tagging invalid bytes with `codec`.
    pub fn into_string_for(self, codec: CodecId) -> Result<String> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Bytes(bytes) => {
                String::from_utf8(bytes).map_err(|err| sim_kernel::Error::CodecError {
                    codec,
                    message: format!("codec input is not valid UTF-8: {err}"),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_utf8_error_uses_supplied_codec_id() {
        let err = Input::Bytes(vec![0xff])
            .into_string_for(CodecId(42))
            .unwrap_err();

        match err {
            sim_kernel::Error::CodecError { codec, message } => {
                assert_eq!(codec, CodecId(42));
                assert!(message.contains("not valid UTF-8"), "{message}");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }
}

/// Rendered output produced by an [`Encoder`]: text or bytes.
///
/// Text codecs emit [`Output::Text`]; binary codecs emit [`Output::Bytes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    /// Rendered text.
    Text(String),
    /// Rendered bytes.
    Bytes(Vec<u8>),
}

impl Output {
    /// Take the output as UTF-8 text, decoding [`Output::Bytes`] and failing
    /// closed with a codec error if the bytes are not valid UTF-8.
    pub fn into_text(self) -> Result<String> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Bytes(bytes) => {
                String::from_utf8(bytes).map_err(|err| sim_kernel::Error::CodecError {
                    codec: CodecId(0),
                    message: err.to_string(),
                })
            }
        }
    }
}

/// The syntactic position a decode is targeting, mirroring the kernel's
/// `EncodePosition`.
///
/// Position is the core idea of the codec contract: a decoder reads the same
/// text differently depending on where its result will land. A codec may, for
/// example, lower forms to calls in [`DecodePosition::Eval`] but keep them as
/// data everywhere else (see [`CodecDefaultDecode::target_for`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodePosition {
    /// Decoding into evaluable position: the result will be evaluated.
    Eval,
    /// Decoding inside a quote: the result is literal structure, not evaluated.
    Quote,
    /// Decoding as plain data.
    Data,
    /// Decoding into a pattern (match/binding) position.
    Pattern,
}

impl From<sim_kernel::EncodePosition> for DecodePosition {
    fn from(position: sim_kernel::EncodePosition) -> Self {
        match position {
            sim_kernel::EncodePosition::Eval => Self::Eval,
            sim_kernel::EncodePosition::Quote => Self::Quote,
            sim_kernel::EncodePosition::Data => Self::Data,
            sim_kernel::EncodePosition::Pattern => Self::Pattern,
        }
    }
}

/// The checked form a decode resolves to once a position is known: inert data
/// or an evaluable term.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeTarget {
    /// Decode to a `Datum` (inert data).
    Datum,
    /// Decode to a `Term` (an evaluable form, e.g. a call).
    Term,
}

/// A codec's policy for choosing a [`DecodeTarget`] from a [`DecodePosition`].
///
/// Data codecs always yield data; eval-aware codecs yield a term in eval
/// position and data otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecDefaultDecode {
    /// Always decode to a `Datum`, regardless of position.
    Datum,
    /// Decode to a `Term` in [`DecodePosition::Eval`], otherwise to a `Datum`.
    TermInEvalDatumOtherwise,
}

impl CodecDefaultDecode {
    /// Resolve the [`DecodeTarget`] for `position` under this policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_codec::{CodecDefaultDecode, DecodePosition, DecodeTarget};
    ///
    /// let policy = CodecDefaultDecode::TermInEvalDatumOtherwise;
    /// assert_eq!(policy.target_for(DecodePosition::Eval), DecodeTarget::Term);
    /// assert_eq!(policy.target_for(DecodePosition::Data), DecodeTarget::Datum);
    ///
    /// // A pure data codec always yields data.
    /// assert_eq!(
    ///     CodecDefaultDecode::Datum.target_for(DecodePosition::Eval),
    ///     DecodeTarget::Datum,
    /// );
    /// ```
    pub fn target_for(self, position: DecodePosition) -> DecodeTarget {
        match (self, position) {
            (Self::TermInEvalDatumOtherwise, DecodePosition::Eval) => DecodeTarget::Term,
            _ => DecodeTarget::Datum,
        }
    }

    /// The stable kebab-case name of this policy, for metadata and display.
    pub fn as_symbol_name(self) -> &'static str {
        match self {
            Self::Datum => "datum",
            Self::TermInEvalDatumOtherwise => "term-in-eval-datum-otherwise",
        }
    }
}

/// The core decode contract: turn [`Input`] into a checked kernel `Expr`.
///
/// Every codec that can read implements `Decoder`. The [`ReadCx`] carries the
/// kernel context, the codec id, the read policy, and the decode limits applied
/// to untrusted input.
pub trait Decoder: Send + Sync {
    /// Decode `input` into a kernel `Expr`, charging the [`ReadCx`] budget.
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr>;
}

/// A decoder that preserves source `Origin`, producing a [`LocatedExpr`].
///
/// Optional: [`CodecRuntime`] falls back to a plain [`Decoder`] with no origin
/// when a codec provides none.
pub trait LocatedDecoder: Send + Sync {
    /// Decode `input` into a [`LocatedExpr`], attributing spans to `source_id`.
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr>;
}

/// A decoder that preserves the full source tree as a [`LocatedExprTree`],
/// including trivia, for lossless round-tripping.
pub trait TreeDecoder: Send + Sync {
    /// Decode `input` into a [`LocatedExprTree`], attributing spans to
    /// `source_id`.
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree>;
}

/// The core encode contract: render a kernel `Expr` to [`Output`].
///
/// Every codec that can write implements `Encoder`. The [`WriteCx`] carries the
/// kernel context, the codec id, and the [`EncodeOptions`](sim_kernel::EncodeOptions)
/// that fix the output position and fidelity.
pub trait Encoder: Send + Sync {
    /// Encode `expr` into [`Output`] under the context's encode options.
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &sim_kernel::Expr) -> Result<Output>;
}

/// An encoder that consumes a [`LocatedExpr`], able to use source origin for a
/// higher-fidelity rendering than the plain [`Encoder`].
pub trait LocatedEncoder: Send + Sync {
    /// Encode a [`LocatedExpr`], optionally using its origin for fidelity.
    fn encode_located(&self, cx: &mut WriteCx<'_>, expr: &LocatedExpr) -> Result<Output>;
}

/// An encoder that consumes a full [`LocatedExprTree`], able to reproduce trivia
/// and exact layout for lossless round-trips.
pub trait TreeEncoder: Send + Sync {
    /// Encode a [`LocatedExprTree`], reproducing layout and trivia where present.
    fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output>;
}

#[sim_citizen_derive::non_citizen(
    reason = "codec runtime registry handle; reconstruct by loading the codec lib and using the codec symbol",
    kind = "handle",
    descriptor = "core/Codec"
)]
/// A registered codec as a runtime object: a symbol-named bundle of optional
/// decode/encode capabilities plus the Shapes and default-decode policy it
/// exposes.
///
/// This is the value the kernel hands back from a codec lookup. Each capability
/// is optional; the dispatch methods ([`decode`](CodecRuntime::decode),
/// [`encode_located`](CodecRuntime::encode_located), ...) pick the richest
/// implementation present and fall back to the plain forms otherwise, failing
/// closed with a codec error when none is provided.
pub struct CodecRuntime {
    /// Stable id of this codec.
    pub id: CodecId,
    /// The symbol the codec is registered and looked up under.
    pub symbol: Symbol,
    /// Plain `Expr` decoder, if the codec can read.
    pub decoder: Option<Arc<dyn Decoder>>,
    /// Origin-preserving decoder, if available.
    pub located_decoder: Option<Arc<dyn LocatedDecoder>>,
    /// Full-tree (lossless) decoder, if available.
    pub tree_decoder: Option<Arc<dyn TreeDecoder>>,
    /// Plain `Expr` encoder, if the codec can write.
    pub encoder: Option<Arc<dyn Encoder>>,
    /// Origin-aware encoder, if available.
    pub located_encoder: Option<Arc<dyn LocatedEncoder>>,
    /// Full-tree (lossless) encoder, if available.
    pub tree_encoder: Option<Arc<dyn TreeEncoder>>,
    /// Advisory browse metadata: a `Shape` that describes the expressions this
    /// codec is intended for, surfaced in the codec's browse table so callers
    /// can discover a codec's domain. It is descriptive only -- decode does not
    /// validate results against it (each codec enforces its own grammar and
    /// [`DecodeLimits`](crate::DecodeLimits) directly).
    pub expr_shape: ShapeRef,
    /// Shape describing this codec's options table.
    pub options_shape: ShapeRef,
    /// How this codec maps a [`DecodePosition`] to a [`DecodeTarget`].
    pub default_decode: CodecDefaultDecode,
}

impl CodecRuntime {
    /// Decode `input` with this codec's plain decoder, erroring if it has none.
    pub fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr> {
        let Some(decoder) = &self.decoder else {
            return Err(sim_kernel::Error::CodecError {
                codec: self.id,
                message: format!("codec {} has no decoder", self.symbol),
            });
        };
        decoder.decode(cx, input)
    }

    /// Encode `expr` with this codec's plain encoder, erroring if it has none.
    pub fn encode(&self, cx: &mut WriteCx<'_>, expr: &sim_kernel::Expr) -> Result<Output> {
        let Some(encoder) = &self.encoder else {
            return Err(sim_kernel::Error::CodecError {
                codec: self.id,
                message: format!("codec {} has no encoder", self.symbol),
            });
        };
        encoder.encode(cx, expr)
    }

    /// Decode `input` preserving origin, falling back to [`decode`](Self::decode)
    /// with no origin when the codec has no [`LocatedDecoder`].
    pub fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr> {
        if let Some(decoder) = &self.located_decoder {
            return decoder.decode_located(cx, input, source_id);
        }
        Ok(LocatedExpr {
            expr: self.decode(cx, input)?,
            origin: None,
        })
    }

    /// Decode `input` into a full tree, falling back to
    /// [`decode_located`](Self::decode_located) reconstructed recursively when
    /// the codec has no [`TreeDecoder`].
    pub fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        if let Some(decoder) = &self.tree_decoder {
            return decoder.decode_tree(cx, input, source_id);
        }
        let located = self.decode_located(cx, input, source_id)?;
        let mut tree = LocatedExprTree::from_expr_recursive(located.expr.clone());
        tree.origin = located.origin;
        Ok(tree)
    }

    /// Encode a [`LocatedExpr`], using the origin-aware encoder only when
    /// lossless-origin output is requested; otherwise drop the origin and use
    /// [`encode`](Self::encode).
    pub fn encode_located(&self, cx: &mut WriteCx<'_>, expr: &LocatedExpr) -> Result<Output> {
        if cx.options.lossless_origin
            && let Some(encoder) = &self.located_encoder
        {
            return encoder.encode_located(cx, expr);
        }
        self.encode(cx, &expr.expr)
    }

    /// Encode a [`LocatedExprTree`], preferring the tree encoder then the
    /// located encoder for lossless-origin output, and otherwise dropping to
    /// [`encode`](Self::encode) on the bare expression.
    pub fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output> {
        if cx.options.lossless_origin {
            if let Some(encoder) = &self.tree_encoder {
                return encoder.encode_tree(cx, expr);
            }
            if let Some(encoder) = &self.located_encoder {
                return encoder.encode_located(cx, &expr.located());
            }
        }
        self.encode(cx, &expr.expr)
    }
}

impl Object for CodecRuntime {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<codec {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for CodecRuntime {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Codec"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_CODEC_CLASS_ID,
            Symbol::qualified("core", "Codec"),
        )
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<sim_kernel::Expr> {
        Ok(sim_kernel::Expr::Symbol(self.symbol.clone()))
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().table(vec![
            (
                Symbol::new("symbol"),
                cx.factory().string(self.symbol.to_string())?,
            ),
            (
                Symbol::new("has-decoder"),
                cx.factory().bool(self.decoder.is_some())?,
            ),
            (
                Symbol::new("has-encoder"),
                cx.factory().bool(self.encoder.is_some())?,
            ),
            (
                Symbol::new("has-located-decoder"),
                cx.factory().bool(self.located_decoder.is_some())?,
            ),
            (
                Symbol::new("has-located-encoder"),
                cx.factory().bool(self.located_encoder.is_some())?,
            ),
            (
                Symbol::new("has-tree-decoder"),
                cx.factory().bool(self.tree_decoder.is_some())?,
            ),
            (
                Symbol::new("has-tree-encoder"),
                cx.factory().bool(self.tree_encoder.is_some())?,
            ),
            (
                Symbol::new("default-decode"),
                cx.factory()
                    .string(self.default_decode.as_symbol_name().to_owned())?,
            ),
            (Symbol::new("expr-shape"), self.expr_shape.clone()),
            (Symbol::new("options-shape"), self.options_shape.clone()),
        ])
    }
}
