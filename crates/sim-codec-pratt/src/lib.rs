//! Shared Pratt parser substrate for SIM codecs.
//!
//! A concrete codec owns its lexer and operator table, then hands a stream of
//! [`SpannedPrattToken`] values to [`PrattCodecParser`]. The parser applies the
//! shared precedence-climbing driver and returns a [`sim_kernel::LocatedExprTree`]
//! with source spans, trivia, and decode-budget checks attached to the parsed
//! expression nodes.
//!
//! The crate is intentionally language-neutral: token sources decide how source
//! text becomes Pratt tokens, while this crate owns only the common grouping and
//! expression-tree construction rules.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod builders;
mod parser;
#[cfg(test)]
mod tests;
mod token;

pub use parser::{PrattCodecParser, raw_number_expr, raw_number_tag};
pub use token::{PrattTokenSource, SpannedPrattToken};

/// Cookbook recipes for this parser substrate, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
