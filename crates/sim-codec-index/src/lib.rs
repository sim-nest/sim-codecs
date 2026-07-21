//! SIM Index codec over the shared `IndexDoc` graph.
//!
//! `sim-codec-index` gives the derived SIM Index one checked codec surface. It
//! decodes canonical s-expression text through the existing Lisp codec into an
//! `Expr`, checks that expression against the index grammar, builds the shared
//! [`IndexDoc`] model, and then runs the graph checks from `sim-index-core`.
//! Encoding starts from the same `IndexDoc` and renders either canonical
//! s-expression text or tagged JSON text from one expression projection.
//!
//! # Example
//!
//! ```
//! use sim_codec_index::{IndexCodec, IndexForm};
//! use sim_index_core::IndexDoc;
//! use sim_kernel::EncodePosition;
//!
//! let codec = IndexCodec;
//! let doc = IndexDoc::public("example");
//! let sx = codec.encode(&doc, EncodePosition::Data, IndexForm::Sx)?;
//! let decoded = codec.decode(IndexForm::Sx, &sx)?;
//!
//! assert_eq!(decoded.schema, "sim.index");
//! # Ok::<(), sim_codec_index::CodecError>(())
//! ```
//!
//! [`IndexDoc`]: sim_index_core::IndexDoc

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
mod expr;
mod form;
mod grammar;
mod runtime;

#[cfg(test)]
mod tests;

pub use error::CodecError;
pub use expr::{expr_from_index_doc, index_doc_from_expr};
pub use form::{IndexForm, decode_index_expr, encode_index_expr};
pub use grammar::{IndexExprShape, index_shape};
pub use runtime::{IndexCodec, IndexCodecLib};
