//! Document domain codec for SIM.
//!
//! Provides `codec:doc`, a domain decoder/encoder pair that turns document
//! text (plain or Markdown) into a semantic markup document `Expr` and back,
//! plus provenance-preserving chunk operations exposed as callable functions.
//! As a domain codec it round-trips only documents and chunks and fails closed
//! outside that domain.
//!
//! Module map (all modules are private; the public surface is re-exported from
//! this crate root):
//! - backend: the `MarkupBackend` trait, deterministic `BackendRegistry`, and
//!   fidelity/error contracts for backend implementations.
//! - codec: the `DocCodec` decoder/encoder, the `DocCodecLib` host lib, and
//!   `install_doc_codec` plus `codec:markup/<id>` installation.
//! - document: compatibility chunking wrappers (`DocValue`, `DocFormat`,
//!   `DocBlock`, `DocChunk`, `ChunkOp`), `decode_document`, and `chunk`.
//! - edit: reversible document-domain edits over `MarkupDoc`.
//! - markup: the shared semantic markup IR (`MarkupDoc`, `MarkupBlock`,
//!   `Inline`) and its ordinary-data projection.
//! - functions: the `doc/chunk-*` chunking functions registered as callables.
//! - markdown: the `pulldown-cmark` Markdown backend.
//! - typst_backend: the `typst-syntax` Typst backend.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod backend;
mod codec;
mod document;
mod edit;
#[cfg(test)]
mod edit_tests;
mod functions;
mod markdown;
mod markdown_writer;
mod markup;
#[cfg(test)]
mod tests;
mod typst_backend;

/// Cookbook recipes embedded from this crate's `recipes/` directory.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

pub use backend::{
    BackendRegistry, BasicMarkdownBackend, MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions,
    MarkupError, MarkupFidelity, MarkupLoss, default_backend_registry,
};
pub use codec::{
    DocCodec, DocCodecLib, MARKUP_CODEC_PREFIX, MarkupCodec, install_doc_codec,
    install_markup_codecs, markup_codec_symbol,
};
pub use document::{
    ChunkOp, DocBlock, DocBlockKind, DocChunk, DocFormat, DocValue, chunk, decode_document,
};
pub use edit::{MarkupEdit, apply_edit, invert_edit};
pub use markdown::MarkdownBackend;
pub use markup::{
    BackendId, Inline, MarkupBlock, MarkupDoc, MathSource, SourceDoc, Span, SpanState,
    decode_markup_doc,
};
pub use typst_backend::TypstBackend;
