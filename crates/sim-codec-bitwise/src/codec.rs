//! The `BitwiseCodec` runtime object, `Lib` registration, and frame functions.
//!
//! Exposes the free `encode_*` / `decode_*` frame helpers and wires the bitwise
//! reader and writer into the codec decoder/encoder, located, and tree roles.

use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, Decoder, Encoder, Input, LocatedDecoder, LocatedEncoder,
    Output, ReadCx, TreeDecoder, TreeEncoder, codec_value, validate_expr_tree,
};
use sim_kernel::{
    AbiVersion, Dependency, Export, Expr, Lib, LibManifest, LibTarget, Linker, LocatedExpr,
    LocatedExprTree, Result, Symbol, Version, WriteCx,
};

use crate::reader::FrameReader;
use crate::types::{FLAG_DENSE, FLAG_NONE, FLAG_ORIGIN, FLAG_TREE_ORIGIN};
use crate::writer::FrameWriter;
use crate::{BitwiseFrame, DecodeLimits, FrameTables};

/// Bitwise codec runtime object: the canonical, minimal sibling of
/// `codec:binary`.
///
/// As a general-purpose expression codec it round-trips kernel `Expr` values as
/// bit-packed, self-delimiting frames, implementing every codec role --
/// [`Decoder`]/[`Encoder`], located [`LocatedDecoder`]/[`LocatedEncoder`], and
/// tree [`TreeDecoder`]/[`TreeEncoder`] -- over the shared `Expr` graph. It
/// fails closed (under [`DecodeLimits`]) on any input that is not a well-formed
/// frame, and its plain-mode output is the smallest canonical byte string for a
/// value. Decoded bytes are treated strictly as data, never as executable input.
pub struct BitwiseCodec;

impl Decoder for BitwiseCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let bytes = input_bytes(input);
        decode_located_tree_frame_with_limits(cx.codec, &bytes, DecodeLimits::from(cx.limits))
            .map(|(_, tree)| tree.located().expr)
    }
}

impl Encoder for BitwiseCodec {
    fn encode(&self, _cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        Ok(Output::Bytes(encode_frame(expr)?.0))
    }
}

impl LocatedDecoder for BitwiseCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExpr> {
        let bytes = input_bytes(input);
        decode_located_tree_frame_with_limits(cx.codec, &bytes, DecodeLimits::from(cx.limits))
            .map(|(_, tree)| tree.located())
    }
}

impl LocatedEncoder for BitwiseCodec {
    fn encode_located(&self, cx: &mut WriteCx<'_>, expr: &LocatedExpr) -> Result<Output> {
        Ok(Output::Bytes(
            encode_located_frame(expr, cx.options.lossless_origin)?.0,
        ))
    }
}

impl TreeDecoder for BitwiseCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        _source_id: String,
    ) -> Result<LocatedExprTree> {
        let bytes = input_bytes(input);
        decode_located_tree_frame_with_limits(cx.codec, &bytes, DecodeLimits::from(cx.limits))
            .map(|(_, tree)| tree)
    }
}

impl TreeEncoder for BitwiseCodec {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output> {
        validate_expr_tree(cx.codec, expr)?;
        Ok(Output::Bytes(
            encode_located_tree_frame(expr, cx.options.lossless_origin)?.0,
        ))
    }
}

fn input_bytes(input: Input) -> Vec<u8> {
    match input {
        Input::Text(text) => text.into_bytes(),
        Input::Bytes(bytes) => bytes,
    }
}

/// Encodes a bare [`Expr`] into a [`BitwiseFrame`], without source origins.
///
/// This is the plain, canonical form: the smallest byte string for the value,
/// suitable as a content-address input (see [`crate::canonical_bytes`]).
pub fn encode_frame(expr: &Expr) -> Result<BitwiseFrame> {
    encode_located_frame(
        &LocatedExpr {
            expr: expr.clone(),
            origin: None,
        },
        false,
    )
}

/// Encodes a bare [`Expr`] into a dense [`BitwiseFrame`] with structural sharing.
///
/// Dense mode assigns every subexpression a pre-order number and, on a
/// value-equal subtree already emitted, writes a `Ref` back-reference tag
/// instead of re-encoding it. It is a deterministic pure function
/// of the value tree and round-trips by [`Expr::canonical_eq`], but it is
/// explicitly opt-in: the plain [`encode_frame`] and [`crate::canonical_bytes`]
/// stay ref-free so the content-addressing key is unchanged.
pub fn encode_dense(expr: &Expr) -> Result<BitwiseFrame> {
    let tables = FrameTables::collect(expr);
    let mut writer = FrameWriter::new(tables);
    writer.flags = FLAG_DENSE;
    writer.set_dense(true);
    writer.write_header()?;
    writer.write_expr(expr)?;
    Ok(BitwiseFrame(writer.finish()))
}

/// Encodes a [`LocatedExpr`] into a [`BitwiseFrame`].
///
/// When `include_origin` is set and `located` carries an origin, the frame is
/// flagged to carry that single origin so the located form round-trips.
pub fn encode_located_frame(located: &LocatedExpr, include_origin: bool) -> Result<BitwiseFrame> {
    let tables = FrameTables::collect(&located.expr);
    let mut writer = FrameWriter::new(tables);
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
    Ok(BitwiseFrame(writer.finish()))
}

/// Encodes a [`LocatedExprTree`] into a [`BitwiseFrame`].
///
/// When `include_origin` is set the frame carries the per-node origin tree so
/// the full located tree round-trips; otherwise only the `Expr` body is
/// written. The tree is validated before encoding and rejected if malformed.
pub fn encode_located_tree_frame(
    tree: &LocatedExprTree,
    include_origin: bool,
) -> Result<BitwiseFrame> {
    validate_expr_tree(sim_kernel::CodecId(0), tree)?;
    let tables = FrameTables::collect(&tree.expr);
    let mut writer = FrameWriter::new(tables);
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
    Ok(BitwiseFrame(writer.finish()))
}

/// Decodes frame `bytes` into its side [`FrameTables`] and bare [`Expr`].
///
/// Any source origins carried by the frame are dropped. Decoding is bounded by
/// the default [`DecodeLimits`] and fails closed on malformed or oversize input.
pub fn decode_frame(codec: sim_kernel::CodecId, bytes: &[u8]) -> Result<(FrameTables, Expr)> {
    let (tables, tree) = decode_located_tree_frame(codec, bytes)?;
    Ok((tables, tree.located().expr))
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
pub fn decode_located_tree_frame(
    codec: sim_kernel::CodecId,
    bytes: &[u8],
) -> Result<(FrameTables, LocatedExprTree)> {
    decode_located_tree_frame_with_limits(codec, bytes, DecodeLimits::default())
}

/// Decodes frame `bytes` into its side [`FrameTables`] and a
/// [`LocatedExprTree`], enforcing the supplied `limits`.
///
/// This is the bounded decode primitive the codec roles call. It rejects an
/// unknown version, unknown/dense flag bits, out-of-range table indices,
/// oversize counts, reserved tags, and any nonzero or byte-sized trailing
/// padding, failing closed on untrusted input.
pub fn decode_located_tree_frame_with_limits(
    codec: sim_kernel::CodecId,
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<(FrameTables, LocatedExprTree)> {
    let mut reader = FrameReader::new(codec, bytes, limits)?;
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
    reader.require_zero_padding()?;
    Ok((tables, tree))
}

/// [`Lib`] that registers the bitwise codec with the runtime.
///
/// Its manifest exports the `codec/bitwise` codec, and loading wires a
/// [`BitwiseCodec`] into the linker as the decode and encode surface for all
/// six codec roles.
pub struct BitwiseCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}

impl BitwiseCodecLib {
    /// Creates the codec lib bound to the runtime-assigned `id` for
    /// `codec/bitwise`.
    pub fn new(id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "bitwise"),
            codec_id: id,
        }
    }
}

impl Lib for BitwiseCodecLib {
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
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "BitwiseFrame"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(BitwiseCodec)),
                located_decoder: Some(Arc::new(BitwiseCodec)),
                tree_decoder: Some(Arc::new(BitwiseCodec)),
                encoder: Some(Arc::new(BitwiseCodec)),
                located_encoder: Some(Arc::new(BitwiseCodec)),
                tree_encoder: Some(Arc::new(BitwiseCodec)),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        Ok(())
    }
}
