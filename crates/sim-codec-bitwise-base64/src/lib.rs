//! Base64 text framing over the SIM bitwise codec.
//!
//! This crate is a thin text wrapper around `sim-codec-bitwise`: it produces and
//! consumes the same bit-packed `Expr` frames, but carries them as a
//! base64-encoded ASCII string so the canonical minimal codec can be used on
//! text-only transports. It is the exact analog of `sim-codec-binary-base64`
//! over the bitwise frame, and it shares that crate's base64 implementation
//! rather than forking it. Encoding delegates to the bitwise codec and
//! base64-encodes the resulting frame; decoding base64-decodes the text and
//! hands the bytes back to the bitwise reader.
//!
//! The public surface is the [`BitwiseBase64Codec`] runtime object, registered
//! via [`BitwiseBase64CodecLib`].
//!
//! # Examples
//!
//! Register the codec and round-trip an [`Expr`] through base64 text: encoding
//! produces an ASCII string, and decoding that string recovers the value.
//!
//! ```
//! use std::sync::Arc;
//! use sim_codec::{Input, decode_with_codec, encode_with_codec};
//! use sim_codec_bitwise_base64::BitwiseBase64CodecLib;
//! use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol};
//!
//! let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
//! sim_test_support::register_core_classes(&mut cx);
//!
//! let lib = BitwiseBase64CodecLib::new(cx.registry_mut().fresh_codec_id());
//! cx.load_lib(&lib)?;
//! let codec = Symbol::qualified("codec", "bitwise-base64");
//!
//! let expr = Expr::List(vec![Expr::Nil, Expr::Bool(true)]);
//! let text = encode_with_codec(&mut cx, &codec, &expr, Default::default())?
//!     .into_text()?;
//! assert!(text.is_ascii());
//!
//! let back = decode_with_codec(&mut cx, &codec, Input::Text(text), ReadPolicy::default())?;
//! assert!(back.canonical_eq(&expr));
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! The wrapped text is untrusted: input that is not valid base64 fails closed
//! rather than being interpreted.
//!
//! ```
//! use std::sync::Arc;
//! use sim_codec::{Input, decode_with_codec};
//! use sim_codec_bitwise_base64::BitwiseBase64CodecLib;
//! use sim_kernel::{Cx, DefaultFactory, EagerPolicy, ReadPolicy, Symbol};
//!
//! let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
//! sim_test_support::register_core_classes(&mut cx);
//! let lib = BitwiseBase64CodecLib::new(cx.registry_mut().fresh_codec_id());
//! cx.load_lib(&lib)?;
//! let codec = Symbol::qualified("codec", "bitwise-base64");
//!
//! let result = decode_with_codec(
//!     &mut cx,
//!     &codec,
//!     Input::Text("not base64!".to_owned()),
//!     ReadPolicy::default(),
//! );
//! assert!(result.is_err());
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! [`Expr`]: sim_kernel::Expr

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod codec;
#[cfg(test)]
mod tests;

pub use codec::{BitwiseBase64Codec, BitwiseBase64CodecLib};

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
