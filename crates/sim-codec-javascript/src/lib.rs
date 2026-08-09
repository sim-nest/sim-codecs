//! Bounded, lossless ECMAScript 2026 Script and Module frontend.
//!
//! The public model is deliberately neutral syntax data. It has no compiler IR,
//! runtime, TypeScript, Shape, or kernel dependency. Parser extensions can wrap
//! [`Node`] and attach their own [`Origin`] without changing JavaScript syntax.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod lexer;
mod parser;
mod types;

pub use lexer::{tokenize, tokenize_with_limits};
pub use parser::{parse_module, parse_module_with_limits, parse_script, parse_script_with_limits};
pub use types::{
    Asi, Diagnostic, DiagnosticCode, Goal, LexicalGoal, Limits, Node, NodeKind, Origin, Span,
    SyntaxTree, Token, TokenKind,
};

/// Frozen ECMA-262 edition.
pub const ECMA262_EDITION: &str = "ECMA-262, 17th edition (ECMAScript 2026)";
/// Immutable specification authority used by this frontend.
pub const ECMA262_AUTHORITY: &str = "https://tc39.es/ecma262/2026/multipage/";
/// Date on which the edition identity and grammar inventory were frozen.
pub const ECMA262_FROZEN_ON: &str = "2026-08-01";
/// Frozen Test262 evidence corpus revision. Test262 is an oracle, not a dependency.
pub const TEST262_REVISION: &str = "tc39/test262@main as observed 2026-08-01";
/// SHA-256 of the checked source corpus manifest.
pub const CORPUS_MANIFEST_SHA256: &str =
    "88b15b687f3741ba7c0145d1e2f09e05f0b1d8bf95656f257656fe3aefe416da";

#[cfg(test)]
mod tests;
