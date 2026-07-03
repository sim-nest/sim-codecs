//! Canonical minimal bit-packed wire codec for the SIM runtime.
//!
//! `codec:bitwise` is the canonical, minimal sibling of `codec:binary`: a
//! framed, fail-closed, general-purpose `Expr` codec whose wire format is
//! bit-granular rather than byte-granular. It implements all six codec roles
//! ([`Decoder`]/[`Encoder`], located, and tree) over the shared `Expr` graph,
//! carries the same interning side tables and [`DecodeLimits`], and adds three
//! density and determinism wins:
//!
//! - **One `vbits` primitive** ("size of the size, then the size") for every
//!   length, index, magnitude, and span, so no leading zero bit is ever emitted.
//! - **Signed minimal-magnitude integers**: any-sign canonical integer encodes
//!   as a sign bit plus its exact significant bits (`-255` costs ~9 bits, not
//!   ~32); only genuine non-integers fall back to canonical text.
//! - **A self-delimiting frame**: a small version/flags prefix, side tables,
//!   body, and optional origin payloads, with no magic and no pad-count. The
//!   final carrier byte is zero-padded and every trailing bit must be zero.
//!
//! Fields pack across byte boundaries, so no field is byte-aligned inside the
//! logical frame. The plain-mode frame is deterministic (canonical map/set
//! order, minimal magnitudes, no padding), which makes [`canonical_bytes`] the
//! smallest canonical byte string for an `Expr` value -- the natural
//! content-address / cassette serialization for the FABRIC_2 / MODEL_2 content
//! store (a documentation pointer only; this crate adds no such dependency).
//!
//! The public carrier is unchanged: the codec reads [`Input::Bytes`] (accepting
//! [`Input::Text`] by UTF-8 bytes for parity with binary) and emits
//! [`Output::Bytes`]. The bit cursor lives entirely inside private
//! reader/writer types.
//!
//! # Measured tradeoff (vs `codec:binary`)
//!
//! The density is real but has a CPU cost, and both are measured by the
//! `sim-codec-compare` harness (run `cargo run --release -p sim-codec-compare
//! --bin report`). As of 2026-07-01: bitwise is ~40-50% smaller than `binary` on
//! structured / integer-dense / realistic data at a modest ~1.0-1.5x
//! encode/decode cost, and `encode_dense` collapses repetitive data to ~0.07-0.14
//! of `binary`. It is never larger than `binary`. The honest non-wins: raw UTF-8
//! strings are a size tie AND up to ~9x slower to encode, so prefer `binary` on
//! the hot path and for string-blob-heavy payloads. Bitwise earns its keep for
//! canonical storage / content-addressing and structured runtime data.
//!
//! # Examples
//!
//! Register the codec on a runtime, round-trip through the codec surface, and
//! confirm the canonical bytes are stable:
//!
//! ```
//! use sim_codec::{Input, decode_with_codec, encode_with_codec};
//! use sim_codec_bitwise::{BitwiseCodecLib, canonical_bytes};
//! use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol};
//!
//! let mut cx = Cx::new(std::sync::Arc::new(EagerPolicy), std::sync::Arc::new(DefaultFactory));
//! sim_test_support::register_core_classes(&mut cx);
//! let lib = BitwiseCodecLib::new(cx.registry_mut().fresh_codec_id());
//! cx.load_lib(&lib)?;
//!
//! let codec = Symbol::qualified("codec", "bitwise");
//! let expr = Expr::List(vec![Expr::Nil, Expr::Bool(true)]);
//! let sim_codec::Output::Bytes(bytes) =
//!     encode_with_codec(&mut cx, &codec, &expr, Default::default())?
//! else { panic!("bitwise emits bytes") };
//! let back = decode_with_codec(&mut cx, &codec, Input::Bytes(bytes), ReadPolicy::default())?;
//! assert!(back.canonical_eq(&expr));
//! assert_eq!(canonical_bytes(&expr)?, canonical_bytes(&back)?); // canonical + stable
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! Arbitrary bytes are untrusted data, not executable input: a frame that does
//! not decode fails closed rather than running anything.
//!
//! ```
//! use sim_codec_bitwise::decode_frame;
//! use sim_kernel::CodecId;
//!
//! assert!(decode_frame(CodecId(1), b"\xff\xff\xff\xff").is_err());
//! ```
//!
//! Opt-in dense mode shares a repeated, value-equal subtree behind a
//! back-reference, so a value with structural repetition encodes strictly
//! smaller than the plain tree while still round-tripping by value. The plain
//! [`canonical_bytes`] stay ref-free:
//!
//! ```
//! use sim_codec_bitwise::{canonical_bytes, decode_frame, encode_dense};
//! use sim_kernel::{CodecId, Expr, Symbol};
//!
//! let shared = Expr::List(vec![
//!     Expr::Symbol(Symbol::qualified("math", "add")),
//!     Expr::String("a repeated leaf payload".to_owned()),
//!     Expr::Bool(true),
//! ]);
//! let expr = Expr::List(vec![shared.clone(), shared.clone(), shared]);
//!
//! let dense = encode_dense(&expr)?;
//! assert!(dense.0.len() < canonical_bytes(&expr)?.len()); // smaller than the plain tree
//!
//! let (_tables, decoded) = decode_frame(CodecId(1), &dense.0)?;
//! assert!(decoded.canonical_eq(&expr));
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! [`Input::Bytes`]: sim_codec::Input::Bytes
//! [`Input::Text`]: sim_codec::Input::Text
//! [`Output::Bytes`]: sim_codec::Output::Bytes
//! [`Decoder`]: sim_codec::Decoder
//! [`Encoder`]: sim_codec::Encoder

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bitio;
mod codec;
mod number;
mod reader;
mod tables;
#[cfg(test)]
mod tests;
mod types;
mod writer;

use sim_kernel::{Expr, Result};

pub use codec::{
    BitwiseCodec, BitwiseCodecLib, decode_frame, decode_located_frame, decode_located_tree_frame,
    decode_located_tree_frame_with_limits, encode_dense, encode_frame, encode_located_frame,
    encode_located_tree_frame,
};
pub use types::{BitwiseFrame, DecodeLimits, FrameTables};

/// Returns the canonical minimal serialization of `expr`: the plain-mode
/// bitwise frame with no origin and no dense references.
///
/// This is the documented smallest canonical byte string for an `Expr` value
/// and is suitable as a `ContentKey` input for a content-addressed store.
/// Structurally equal values (including maps and sets in any insertion order)
/// produce identical bytes, and re-encoding a decoded frame is idempotent.
pub fn canonical_bytes(expr: &Expr) -> Result<Vec<u8>> {
    Ok(encode_frame(expr)?.0)
}
