//! Parsing root for the Algol codec, aggregating its `tokenize`, `state`,
//! `origin`, and `rewrite` submodules and exposing the `decode_algol_located`
//! decode entry points built on top of the Pratt parser.

mod origin;
mod rewrite;
mod state;
mod tokenize;

use crate::pratt::{PrattParser, default_pratt_table};
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{Expr, LocatedExpr, PrattTable, Result};

pub(crate) use origin::{extend_tree_trivia, tree_origin, with_origin_span};
pub use state::ParseCx;
pub use tokenize::{SpannedToken, tokenize_algol_spanned, tokenize_algol_spanned_with_budget};

pub(crate) use rewrite::{raw_number_expr, raw_number_tag};

/// Decodes Algol source into a [`LocatedExpr`], attaching span and trivia
/// origin so source layout round-trips.
///
/// Uses [`crate::default_pratt_table`] and a default decode budget; call
/// [`decode_algol_located_with_budget`] to supply an explicit budget. The
/// default [`DecodeLimits::max_input_bytes`] ceiling is applied to `source`
/// before parsing, so this convenience entry point is bounded even when called
/// directly. Raw number literals are carried as a tagged extension form and
/// lowered to concrete number domains later by the runtime decoder.
pub fn decode_algol_located(
    codec: sim_kernel::CodecId,
    source_id: impl Into<String>,
    source: &str,
) -> Result<LocatedExpr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    budget.check_input_bytes(codec, source.len())?;
    decode_algol_located_with_budget(codec, source_id, source, &mut budget)
}

/// Decodes Algol source into a [`LocatedExpr`] under an explicit decode
/// `budget`.
///
/// The budget bounds token count, nesting depth, and string/trivia sizes so a
/// hostile input cannot exhaust resources. Otherwise behaves like
/// [`decode_algol_located`].
pub fn decode_algol_located_with_budget(
    codec: sim_kernel::CodecId,
    source_id: impl Into<String>,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<LocatedExpr> {
    let parser = PrattParser::new(default_pratt_table());
    let source_id = sim_kernel::SourceId(source_id.into());
    let mut tree =
        parser.parse_text_tree_with_budget(codec, source_id.0.clone(), source, budget)?;
    tree.origin = Some(origin::origin_from_algol_source(codec, source_id, source)?);
    Ok(tree.located())
}

/// Parses Algol `source` into a bare [`Expr`] using a caller-supplied operator
/// `table`, lowering raw number literals through `cx`.
///
/// This is the entry point the `Shape` engine uses to parse infix grammar with
/// a custom operator table rather than [`crate::default_pratt_table`]. It
/// applies the default [`DecodeLimits::max_input_bytes`] ceiling to `source`
/// before parsing; call [`parse_algol_expr_with_table_and_budget`] to honor a
/// caller-supplied budget. Number literals are lowered lossily: a literal no
/// number domain accepts is left as the tagged raw form rather than raising an
/// error.
///
/// # Examples
///
/// ```
/// use sim_codec_algol::{default_pratt_table, parse_algol_expr_with_table};
/// use sim_kernel::Expr;
/// use sim_test_support::{core_cx, register_f64_number_domain};
///
/// let mut cx = core_cx();
/// register_f64_number_domain(&mut cx);
/// let expr = parse_algol_expr_with_table(&mut cx, default_pratt_table(), "1 + 2 * 3").unwrap();
/// assert!(matches!(expr, Expr::Infix { .. }));
/// ```
pub fn parse_algol_expr_with_table(
    cx: &mut sim_kernel::Cx,
    table: PrattTable,
    source: &str,
) -> Result<Expr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    budget.check_input_bytes(sim_kernel::CodecId(0), source.len())?;
    parse_algol_expr_with_table_and_budget(cx, table, source, &mut budget)
}

/// Parses Algol `source` into a bare [`Expr`] under an explicit decode `budget`,
/// using a caller-supplied operator `table`.
///
/// Identical to [`parse_algol_expr_with_table`] but honors caller limits rather
/// than hardcoding [`DecodeLimits::default`], so a Shape grammar driven from a
/// limited runtime context bounds the same way the runtime decode path does.
pub fn parse_algol_expr_with_table_and_budget(
    cx: &mut sim_kernel::Cx,
    table: PrattTable,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let mut tree = PrattParser::new(table).parse_text_tree_with_budget(
        sim_kernel::CodecId(0),
        "<shape>",
        source,
        budget,
    )?;
    rewrite::rewrite_number_domains_tree_lossy(cx, &mut tree)?;
    Ok(tree.expr)
}
