//! Side-table collection and small shared helpers.
//!
//! Walks an `Expr` to gather the interned libs, symbols, and number domains
//! that make up a frame's [`FrameTables`]. The traversal order matches
//! `sim-codec-binary` (the logic is lifted, not depended upon) so the two
//! codecs agree on interning.

use std::collections::BTreeSet;

use sim_kernel::{Expr, QuoteMode, Symbol};

use crate::FrameTables;

impl FrameTables {
    pub(crate) fn collect(expr: &Expr) -> Self {
        let mut libs = BTreeSet::new();
        let mut symbols = BTreeSet::new();
        let mut number_domains = BTreeSet::new();
        collect_expr(expr, &mut libs, &mut symbols, &mut number_domains);
        Self {
            libs: libs.into_iter().collect(),
            symbols: symbols.into_iter().collect(),
            number_domains: number_domains.into_iter().collect(),
        }
    }
}

fn collect_expr(
    expr: &Expr,
    libs: &mut BTreeSet<String>,
    symbols: &mut BTreeSet<Symbol>,
    number_domains: &mut BTreeSet<Symbol>,
) {
    match expr {
        Expr::Nil | Expr::Bool(_) | Expr::String(_) | Expr::Bytes(_) => {}
        Expr::Number(number) => {
            collect_symbol(&number.domain, libs, number_domains);
        }
        Expr::Symbol(symbol) => collect_symbol(symbol, libs, symbols),
        Expr::Local(symbol) => collect_symbol(symbol, libs, symbols),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            for item in items {
                collect_expr(item, libs, symbols, number_domains);
            }
        }
        Expr::Map(entries) => {
            for (key, value) in entries {
                collect_expr(key, libs, symbols, number_domains);
                collect_expr(value, libs, symbols, number_domains);
            }
        }
        Expr::Call { operator, args } => {
            collect_expr(operator, libs, symbols, number_domains);
            for arg in args {
                collect_expr(arg, libs, symbols, number_domains);
            }
        }
        Expr::Infix {
            operator,
            left,
            right,
        } => {
            collect_symbol(operator, libs, symbols);
            collect_expr(left, libs, symbols, number_domains);
            collect_expr(right, libs, symbols, number_domains);
        }
        Expr::Prefix { operator, arg } | Expr::Postfix { operator, arg } => {
            collect_symbol(operator, libs, symbols);
            collect_expr(arg, libs, symbols, number_domains);
        }
        Expr::Quote { expr, .. } => collect_expr(expr, libs, symbols, number_domains),
        Expr::Annotated { expr, annotations } => {
            collect_expr(expr, libs, symbols, number_domains);
            for (key, value) in annotations {
                collect_symbol(key, libs, symbols);
                collect_expr(value, libs, symbols, number_domains);
            }
        }
        Expr::Extension { tag, payload } => {
            collect_symbol(tag, libs, symbols);
            collect_expr(payload, libs, symbols, number_domains);
        }
    }
}

fn collect_symbol(symbol: &Symbol, libs: &mut BTreeSet<String>, set: &mut BTreeSet<Symbol>) {
    if let Some(namespace) = &symbol.namespace {
        libs.insert(namespace.to_string());
    }
    set.insert(symbol.clone());
}

/// The 3-bit wire code for a quote mode.
pub(crate) fn quote_mode_bits(mode: QuoteMode) -> u128 {
    match mode {
        QuoteMode::Quote => 0,
        QuoteMode::QuasiQuote => 1,
        QuoteMode::Unquote => 2,
        QuoteMode::Splice => 3,
        QuoteMode::Syntax => 4,
    }
}

/// Inverts [`quote_mode_bits`]; `None` for an unassigned code.
pub(crate) fn quote_mode_from_bits(bits: u128) -> Option<QuoteMode> {
    let mode = match bits {
        0 => QuoteMode::Quote,
        1 => QuoteMode::QuasiQuote,
        2 => QuoteMode::Unquote,
        3 => QuoteMode::Splice,
        4 => QuoteMode::Syntax,
        _ => return None,
    };
    Some(mode)
}
