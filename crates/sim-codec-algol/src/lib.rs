//! General-purpose Algol codec for the SIM runtime: the infix, Pratt-parsed
//! surface that round-trips every expression through the shared `Expr` graph.
//!
//! A decoder tokenizes infix source and parses it with a precedence-driven
//! Pratt parser into checked `Expr` forms; an encoder serializes any `Expr`
//! back to infix text, inserting parentheses according to operator binding
//! power. Like the Lisp codec, it covers the full expression graph rather than
//! a single domain, so any value the kernel can hold round-trips through it.
//!
//! # Module map
//!
//! All submodules are private; their public items are re-exported at the crate
//! root. `parse` tokenizes and decodes infix source into located expression
//! trees and exposes `ParseCx`, `SpannedToken`, and the `decode_algol_located`
//! family. `pratt` holds the precedence-climbing parser (`PrattParser`) and the
//! operator table (`default_pratt_table`, `supports_pratt`). `encode` renders
//! any `Expr` back to infix text (`encode_algol`). `runtime` provides the `Lib`
//! registration (`AlgolCodec`, `AlgolCodecLib`) that wires the codec into the
//! runtime.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod encode;
mod parse;
mod pratt;
mod runtime;

pub use encode::encode_algol;
pub use parse::{
    ParseCx, SpannedToken, decode_algol_located, decode_algol_located_with_budget,
    parse_algol_expr_with_table, parse_algol_expr_with_table_and_budget, tokenize_algol_spanned,
    tokenize_algol_spanned_with_budget,
};
pub use pratt::{PrattParser, default_pratt_table, supports_pratt};
pub use runtime::{AlgolCodec, AlgolCodecLib};

#[cfg(test)]
mod tests;
