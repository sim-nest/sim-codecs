//! General-purpose Lisp codec for the SIM runtime: the s-expression surface
//! that round-trips every expression through the shared `Expr` graph.
//!
//! A decoder lexes and reads parenthesized s-expression text into checked
//! `Expr` forms; an encoder serializes any `Expr` back to Lisp text aware of
//! its output position (eval, quote, data, pattern). Because the codec covers
//! the full expression graph rather than a single domain, it can faithfully
//! represent any value the kernel can hold.
//!
//! # Module map
//!
//! The crate's behavior lives behind the private `implementation` module, whose
//! public items are re-exported at the crate root. Internally that module
//! aggregates: `lex` (tokenizing Lisp source into tokens and trivia), `tree`
//! (reading a token stream into a located expression tree), `decode`
//! (the `Decoder`/`TreeDecoder`/`LocatedDecoder` entry points and surface
//! lowering), `forms` (parsing individual atoms, literals, symbols, logic
//! variables, and quote forms), `encode` (the `Encoder`/`TreeEncoder` rendering
//! of `Expr` back to text), and `runtime` (the `Lib` registration wiring the
//! codec into the runtime). `RECIPES` exposes the embedded cookbook recipes.
//!
//! # Examples
//!
//! Register the codec, decode s-expression text into an [`Expr`], then encode an
//! `Expr` back to Lisp text:
//!
//! ```
//! use std::sync::Arc;
//! use sim_codec::{Input, decode_with_codec, encode_with_codec};
//! use sim_codec_lisp::LispCodecLib;
//! use sim_kernel::{
//!     Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol,
//! };
//!
//! let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
//! sim_test_support::register_core_classes(&mut cx);
//! sim_test_support::register_f64_number_domain(&mut cx);
//!
//! let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id())?;
//! cx.load_lib(&lib)?;
//! let lisp = Symbol::qualified("codec", "lisp");
//!
//! // Decode text into a checked `Expr` form.
//! let expr = decode_with_codec(
//!     &mut cx,
//!     &lisp,
//!     Input::Text("(quote [1 2])".to_owned()),
//!     ReadPolicy::default(),
//! )?;
//! assert!(matches!(expr, Expr::Quote { .. }));
//!
//! // Encode the `Expr` back to Lisp text (a semantic round-trip).
//! let text = encode_with_codec(&mut cx, &lisp, &expr, Default::default())?
//!     .into_text()
//!     .unwrap();
//! assert_eq!(text, "(quote [1 2])");
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! The loadable codec lib also exports `cli/main/codec-lisp`. That entrypoint
//! accepts the standard CLI envelope table, evaluates exactly one source from
//! `eval`, `script`, or `stdin` through the active context eval policy, and
//! returns a `cli/repl` marker for a bare handoff.
//!
//! [`Expr`]: sim_kernel::Expr
#![deny(unsafe_code)]
#![deny(missing_docs)]

mod grammar;
mod implementation;
#[cfg(feature = "native-export")]
mod loaders;
#[cfg(feature = "native-export")]
mod native;
#[cfg(feature = "native-export")]
extern crate self as sim;

#[cfg(feature = "native-export")]
use sim_codec as codec;
#[cfg(feature = "native-export")]
use sim_codec_binary as codec_binary;
#[cfg(feature = "native-export")]
use sim_kernel as kernel;
#[cfg(feature = "native-export")]
use sim_macros::{sim_codec, sim_lib};

/// Cookbook recipes for the Lisp codec, embedded at build time from the crate's
/// `recipes/` directory and exposed for help and browse surfaces.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

pub use grammar::LispGrammarRenderer;
pub use implementation::*;
