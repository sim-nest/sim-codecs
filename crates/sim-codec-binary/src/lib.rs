//! Binary wire codec for the SIM runtime.
//!
//! This crate encodes and decodes kernel `Expr` values as a compact, tagged
//! binary frame. A frame begins with a magic/version header and side tables
//! (interned libs, symbols, and number domains), followed by a tag-prefixed
//! body that walks the `Expr` graph; an optional flag carries source origins so
//! that located expressions and `LocatedExprTree` values round-trip as well.
//!
//! The public surface is the [`BinaryCodec`] runtime object (registered via
//! [`BinaryCodecLib`]) together with the free `encode_*` / `decode_*` frame
//! functions and the frame value types ([`BinaryFrame`], [`BinaryTag`],
//! [`FrameTables`], [`DecodeLimits`]). Decoding is bounded by [`DecodeLimits`]
//! to fail closed on hostile or malformed input.
//!
//! # Examples
//!
//! Encode an [`Expr`] into a [`BinaryFrame`] and decode it straight back,
//! recovering both the value and its interning side tables -- no runtime
//! context is needed for the free frame functions:
//!
//! ```
//! use sim_codec_binary::{BinaryFrame, decode_frame, encode_frame};
//! use sim_kernel::{CodecId, Expr, Symbol};
//!
//! let expr = Expr::List(vec![
//!     Expr::Symbol(Symbol::qualified("math", "add")),
//!     Expr::Bool(true),
//! ]);
//!
//! let BinaryFrame(bytes) = encode_frame(&expr)?;
//! let (tables, back) = decode_frame(CodecId(1), &bytes)?;
//!
//! assert_eq!(back, expr);
//! assert!(tables.symbols.contains(&Symbol::qualified("math", "add")));
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! Arbitrary bytes are untrusted data, not executable input: a frame that does
//! not start with the magic header fails closed rather than running anything.
//!
//! ```
//! use sim_codec_binary::decode_frame;
//! use sim_kernel::CodecId;
//!
//! assert!(decode_frame(CodecId(1), b"not a frame").is_err());
//! ```
//!
//! Register the codec on a runtime and round-trip through the codec surface:
//!
//! ```
//! use std::sync::Arc;
//! use sim_codec::{Input, Output, decode_with_codec, encode_with_codec};
//! use sim_codec_binary::BinaryCodecLib;
//! use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol};
//!
//! let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
//! sim_test_support::register_core_classes(&mut cx);
//!
//! let lib = BinaryCodecLib::new(cx.registry_mut().fresh_codec_id());
//! cx.load_lib(&lib)?;
//! let binary = Symbol::qualified("codec", "binary");
//!
//! let bytes = match encode_with_codec(&mut cx, &binary, &Expr::Bool(true), Default::default())? {
//!     Output::Bytes(bytes) => bytes,
//!     Output::Text(text) => text.into_bytes(),
//! };
//! let back = decode_with_codec(&mut cx, &binary, Input::Bytes(bytes), ReadPolicy::default())?;
//! assert_eq!(back, Expr::Bool(true));
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! [`Expr`]: sim_kernel::Expr

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod codec;
mod reader;
mod tables;
#[cfg(test)]
mod tests;
mod types;
mod writer;

pub(crate) use types::{FLAG_NONE, FLAG_ORIGIN, FLAG_TREE_ORIGIN, MAGIC, VERSION};

pub use codec::{
    BinaryCodec, BinaryCodecLib, decode_frame, decode_located_frame, decode_located_tree_frame,
    decode_located_tree_frame_with_limits, encode_frame, encode_located_frame,
    encode_located_tree_frame,
};
pub use types::{BinaryFrame, BinaryTag, DecodeLimits, FrameTables};

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
