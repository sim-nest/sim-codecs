//! Bounded, lossless Python 3.14 syntax and general-purpose codec.
//!
//! This crate deliberately stops at syntax. [`parse_module`] returns a concrete
//! source tree whose leaves partition the original bytes; it never creates
//! runtime instructions or evaluates Python code.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod codec;
mod grammar;
mod lexer;
mod lower;
mod parser;
mod types;

pub use codec::{PythonCodec, PythonCodecLib};
pub use grammar::{CORPUS_SHA256, GRAMMAR_SHA256, PYTHON_VERSION, frozen_productions};
pub use lexer::{tokenize, tokenize_with_limits};
pub use lower::{
    decode_python, decode_python_located, decode_python_tree, encode_python, lower_python,
};
pub use parser::{parse_module, parse_module_with_limits};
pub use types::{
    Diagnostic, DiagnosticCode, Limits, Node, NodeKind, Span, SyntaxTree, Token, TokenKind,
};

/// Stable local id used before a host assigns `codec/python` an id.
pub const PYTHON_CODEC_ID: sim_kernel::CodecId = sim_kernel::CodecId(0x50_59_00_00);

/// Cookbook recipes embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
