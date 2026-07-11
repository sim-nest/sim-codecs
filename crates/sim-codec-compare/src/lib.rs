//! Developer harness: `sim-codec-bitwise` vs `sim-codec-binary`, measured.
//!
//! The BITWISE family shipped a bit-packed `Expr` codec on the premise that
//! bit-granular packing buys density worth having, without ever measuring it
//! against the byte-granular `codec:binary` baseline. This crate is that
//! measurement. It owns a categorized [`corpus`] of `Expr` values, a [`size`]
//! measurer (bytes per codec/mode), a dependency-free [`speed`] timing harness,
//! and a `report` binary that prints both tables. The findings -- and the honest
//! answer to "when is bitwise actually worth it?" -- live in this crate's README.
//!
//! This is a developer analysis tool, not a runtime library; it is
//! `publish = false`.
//!
//! ```
//! use sim_codec_compare::{corpus::corpus, size::measure_size};
//! let sample = &corpus()[0];
//! let sizes = measure_size(&sample.expr);
//! assert!(sizes.binary > 0 && sizes.bitwise > 0);
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod corpus;
pub mod report;
pub mod size;
pub mod speed;

#[cfg(test)]
mod findings_tests;
#[cfg(test)]
mod speed_smoke_tests;

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
