//! The `BinaryCodec` runtime object, `Lib` registration, and frame functions.
//!
//! Exposes the free `encode_*` / `decode_*` frame helpers and wires the binary
//! reader and writer into the codec decoder/encoder, located, and tree traits.

use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, Decoder, Encoder, Input, LocatedDecoder, LocatedEncoder,
    Output, ReadCx, TreeDecoder, TreeEncoder, codec_value, validate_expr_tree,
};
use sim_kernel::{
    AbiVersion, DefaultFactory, Dependency, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LocatedExpr, LocatedExprTree, Result, Symbol, Version, WriteCx,
};

use crate::cookbook::{BinaryRoundtripReport, roundtrip_report_symbol};
use crate::reader::BinaryReader;
use crate::writer::BinaryWriter;
use crate::{BinaryFrame, DecodeLimits, FLAG_NONE, FLAG_ORIGIN, FLAG_TREE_ORIGIN, FrameTables};

/// Binary codec runtime object that round-trips kernel `Expr` values as compact
/// tagged frames.
///
/// As a domain codec it speaks exactly its own byte frame format: it implements
/// every codec role -- [`Decoder`]/[`Encoder`], located
/// [`LocatedDecoder`]/[`LocatedEncoder`], and tree
/// [`TreeDecoder`]/[`TreeEncoder`] -- over the shared `Expr` graph, and fails
/// closed (under [`DecodeLimits`]) on any input that is not a well-formed frame.
/// Decoded bytes are treated strictly as data, never as executable input.
pub struct BinaryCodec;

impl Decoder for BinaryCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let bytes = match input {
            Input::Text(text) => text.into_bytes(),
            Input::Bytes(bytes) => bytes,
        };
        decode_located_tree_frame_with_limits(cx.codec, &bytes, DecodeLimits::from(cx.limits))
            .map(|(_, tree)| tree.located().expr)
    }
}

impl Encoder for BinaryCodec {
    fn encode(&self, _cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        Ok(Output::Bytes(encode_frame(expr)?.0))
    }
}

impl LocatedDecoder for BinaryCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExpr> {
        let bytes = match input {
            Input::Text(text) => text.into_bytes(),
            Input::Bytes(bytes) => bytes,
        };
        decode_located_tree_frame_with_limits(cx.codec, &bytes, DecodeLimits::from(cx.limits))
            .map(|(_, tree)| tree.located())
    }
}

impl LocatedEncoder for BinaryCodec {
    fn encode_located(&self, cx: &mut WriteCx<'_>, expr: &LocatedExpr) -> Result<Output> {
        Ok(Output::Bytes(
            encode_located_frame(expr, cx.options.lossless_origin)?.0,
        ))
    }
}

impl TreeDecoder for BinaryCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExprTree> {
        let bytes = match input {
            Input::Text(text) => text.into_bytes(),
            Input::Bytes(bytes) => bytes,
        };
        decode_located_tree_frame_with_limits(cx.codec, &bytes, DecodeLimits::from(cx.limits))
            .map(|(_, tree)| tree)
    }
}

impl TreeEncoder for BinaryCodec {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output> {
        validate_expr_tree(cx.codec, expr)?;
        Ok(Output::Bytes(
            encode_located_tree_frame(expr, cx.options.lossless_origin)?.0,
        ))
    }
}

/// Encodes a bare [`Expr`] into a [`BinaryFrame`], without source origins.
pub fn encode_frame(expr: &Expr) -> Result<BinaryFrame> {
    encode_located_frame(
        &LocatedExpr {
            expr: expr.clone(),
            origin: None,
        },
        false,
    )
}

/// Encodes a [`LocatedExpr`] into a [`BinaryFrame`].
///
/// When `include_origin` is set and `located` carries an origin, the frame is
/// flagged to carry that single origin so the located form round-trips.
pub fn encode_located_frame(located: &LocatedExpr, include_origin: bool) -> Result<BinaryFrame> {
    let tables = FrameTables::collect(&located.expr);
    let mut writer = BinaryWriter::new(tables)?;
    writer.flags = if include_origin && located.origin.is_some() {
        FLAG_ORIGIN
    } else {
        FLAG_NONE
    };
    writer.write_header()?;
    writer.write_expr(&located.expr)?;
    if writer.flags & FLAG_ORIGIN != 0 {
        writer.write_origin(
            located
                .origin
                .as_ref()
                .expect("origin flag requires origin payload"),
        )?;
    }
    Ok(BinaryFrame(writer.bytes))
}

/// Encodes a [`LocatedExprTree`] into a [`BinaryFrame`].
///
/// When `include_origin` is set the frame carries the per-node origin tree so
/// that the full located tree round-trips; otherwise only the `Expr` body is
/// written. The tree is validated before encoding and rejected if malformed.
pub fn encode_located_tree_frame(
    tree: &LocatedExprTree,
    include_origin: bool,
) -> Result<BinaryFrame> {
    validate_expr_tree(sim_kernel::CodecId(0), tree)?;
    let tables = FrameTables::collect(&tree.expr);
    let mut writer = BinaryWriter::new(tables)?;
    writer.flags = if include_origin {
        FLAG_TREE_ORIGIN
    } else {
        FLAG_NONE
    };
    writer.write_header()?;
    writer.write_expr(&tree.expr)?;
    if writer.flags & FLAG_TREE_ORIGIN != 0 {
        writer.write_origin_tree(tree)?;
    }
    Ok(BinaryFrame(writer.bytes))
}

/// Decodes frame `bytes` into its side [`FrameTables`] and bare [`Expr`].
///
/// Any source origins carried by the frame are dropped. Decoding is bounded by
/// the default [`DecodeLimits`] and fails closed on malformed or oversize input.
pub fn decode_frame(codec: sim_kernel::CodecId, bytes: &[u8]) -> Result<(FrameTables, Expr)> {
    let located = decode_located_frame(codec, bytes)?;
    Ok((located.0, located.1.expr))
}

/// Decodes frame `bytes` into its side [`FrameTables`] and a [`LocatedExpr`].
///
/// The top-level origin is recovered when the frame carries one. Decoding is
/// bounded by the default [`DecodeLimits`] and fails closed on bad input.
pub fn decode_located_frame(
    codec: sim_kernel::CodecId,
    bytes: &[u8],
) -> Result<(FrameTables, LocatedExpr)> {
    let (tables, tree) = decode_located_tree_frame(codec, bytes)?;
    Ok((tables, tree.located()))
}

/// Decodes frame `bytes` into its side [`FrameTables`] and a full
/// [`LocatedExprTree`], using the default [`DecodeLimits`].
///
/// The per-node origin tree is recovered when the frame carries one. This is
/// the most complete decode entry point; see
/// [`decode_located_tree_frame_with_limits`] to supply explicit limits.
pub fn decode_located_tree_frame(
    codec: sim_kernel::CodecId,
    bytes: &[u8],
) -> Result<(FrameTables, LocatedExprTree)> {
    decode_located_tree_frame_with_limits(codec, bytes, DecodeLimits::default())
}

/// Decodes frame `bytes` into its side [`FrameTables`] and a
/// [`LocatedExprTree`], enforcing the supplied `limits`.
///
/// This is the bounded decode primitive the codec roles call. It rejects bad
/// magic/version/flags, out-of-range table indices, oversize counts, and any
/// trailing bytes after the payload, failing closed on untrusted input.
pub fn decode_located_tree_frame_with_limits(
    codec: sim_kernel::CodecId,
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<(FrameTables, LocatedExprTree)> {
    let mut reader = BinaryReader::new(codec, bytes, limits)?;
    let tables = reader.read_header()?;
    let expr = reader.read_expr()?;
    let mut tree = if reader.flags & FLAG_TREE_ORIGIN != 0 {
        reader.read_origin_tree(expr)?
    } else {
        LocatedExprTree::from_expr_recursive(expr)
    };
    if reader.flags & FLAG_ORIGIN != 0 {
        tree.origin = Some(reader.read_origin()?);
    }
    if !reader.is_empty() {
        return Err(Error::CodecError {
            codec,
            message: "trailing bytes after binary payload".to_owned(),
        });
    }
    Ok((tables, tree))
}

/// [`Lib`] that registers the binary codec with the runtime.
///
/// Its manifest exports the `codec/binary` codec, and loading wires a
/// [`BinaryCodec`] into the linker as the decode and encode surface for all
/// codec roles.
pub struct BinaryCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}

impl BinaryCodecLib {
    /// Creates the codec lib bound to the runtime-assigned `id` for
    /// `codec/binary`.
    pub fn new(id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "binary"),
            codec_id: id,
        }
    }
}

impl Lib for BinaryCodecLib {
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
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "BinaryFrame"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(BinaryCodec)),
                located_decoder: Some(Arc::new(BinaryCodec)),
                tree_decoder: Some(Arc::new(BinaryCodec)),
                encoder: Some(Arc::new(BinaryCodec)),
                located_encoder: Some(Arc::new(BinaryCodec)),
                tree_encoder: Some(Arc::new(BinaryCodec)),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        linker.function_value(
            roundtrip_report_symbol(),
            cx.factory().opaque(Arc::new(BinaryRoundtripReport))?,
        )?;
        Ok(())
    }
}
