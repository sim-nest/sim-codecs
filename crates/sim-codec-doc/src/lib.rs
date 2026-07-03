//! Document domain codec for SIM.
//!
//! Provides `codec:doc`, a domain decoder/encoder pair that turns document
//! text (plain or Markdown) into a structured document `Expr` and back, plus
//! provenance-preserving chunk operations exposed as callable functions. As a
//! domain codec it round-trips only documents and chunks and fails closed
//! outside that domain.
//!
//! Module map (all modules are private; the public surface is re-exported from
//! this crate root):
//! - codec: the `DocCodec` decoder/encoder, the `DocCodecLib` host lib, and
//!   `install_doc_codec`.
//! - document: the document model (`DocValue`, `DocFormat`, `DocBlock`,
//!   `DocChunk`, `ChunkOp`), `decode_document`, and the `chunk` operation.
//! - functions: the `doc/chunk-*` chunking functions registered as callables.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod codec;
mod document;
mod functions;
#[cfg(test)]
mod tests;

/// Cookbook recipes embedded from this crate's `recipes/` directory.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

pub use codec::{DocCodec, DocCodecLib, install_doc_codec};
pub use document::{
    ChunkOp, DocBlock, DocBlockKind, DocChunk, DocFormat, DocValue, chunk, decode_document,
};
