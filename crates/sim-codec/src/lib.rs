//! Core decoder/encoder and output-position contracts for the SIM codec layer.
//!
//! sim-codec is the foundation crate of the sim-codec-* family. The SIM kernel
//! defines the `Expr` graph and the codec contract types; this crate provides
//! the concrete runtime that every other codec crate implements against: the
//! `Decoder`/`Encoder` traits, their located and tree-shaped variants, the
//! `DecodePosition`/`DecodeTarget` output-position model (eval, quote, data,
//! pattern), and the `CodecRuntime` glue that registers a codec as a
//! runtime object. On top of those contracts it carries the shared
//! `Expr`<->tree encode machinery, domain-codec scaffolding, decode resource
//! limits, portable encode/decode, string-literal coding, and tree validation.
//!
//! A decoder turns tokens or text into a checked `Expr` form; an encoder knows
//! its output position and renders an `Expr` back to text or bytes. Downstream
//! crates (general-purpose and domain codecs) build on these primitives rather
//! than re-implementing them.
//!
//! Module map (these modules are private; their public items are re-exported at
//! the crate root, so they are listed here in plain text rather than linked):
//!
//! - implementation: implementation root that aggregates the submodules below
//!   and re-exports their public surface.
//!   - runtime: the `Decoder`/`Encoder` runtime contracts, the
//!     `DecodePosition`/`DecodeTarget` output positions, `Input`/`Output`, and
//!     the `CodecRuntime` registration glue.
//!   - domain: builder scaffolding for domain codec libs (`DomainCodecLib`) and
//!     the shared UTF-8 input helper.
//!   - domain_form: a generic `#(...)` domain-form parser and formatter.
//!   - limits: decode resource ceilings (`DecodeLimits`), running budgets
//!     (`DecodeBudget`), and the `ReadCx` decode context.
//!   - list_encode: the shared list/`Expr` value-to-expr encode machinery.
//!   - lowering: structure-preserving operator-node lowering.
//!   - portable: codec-neutral, lossless portable encode/decode for the data
//!     subset of `Expr`.
//!   - strings: string-literal encode/decode helpers.
//!   - tree: structural validation of a `LocatedExprTree`.
//! - runtime_api: the eval-facing surface that drives codecs through the kernel
//!   (`DecodedForm`, codec lookup, and decode/encode entry points).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod grammar_check;
mod implementation;
mod prism;
mod runtime_api;

pub use grammar_check::*;
pub use implementation::*;
pub use prism::*;
pub use runtime_api::*;

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
