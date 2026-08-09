//! Bounded, lossless Python 3.14 syntax frontend.
//!
//! This crate deliberately stops at syntax. [`parse_module`] returns a concrete
//! source tree whose leaves partition the original bytes; it never creates
//! runtime instructions or evaluates Python code.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod grammar;
mod lexer;
mod parser;
mod types;

pub use grammar::{CORPUS_SHA256, GRAMMAR_SHA256, PYTHON_VERSION, frozen_productions};
pub use lexer::{tokenize, tokenize_with_limits};
pub use parser::{parse_module, parse_module_with_limits};
pub use types::{
    Diagnostic, DiagnosticCode, Limits, Node, NodeKind, Span, SyntaxTree, Token, TokenKind,
};

/// Cookbook recipes embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
