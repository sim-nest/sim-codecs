//! Core of the Pratt parser: `PrattParser` and the precedence-climbing driver
//! that tokenizes infix source and parses it into located expression trees
//! using operator binding powers from the table.

use crate::parse::{
    ParseCx, SpannedToken, extend_tree_trivia, raw_number_expr, tokenize_algol_spanned_with_budget,
    tree_origin, with_origin_span,
};
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{
    Error, Expr, Fixity, LocatedExprTree, PrattOperator, PrattTable, PrattToken as Token, Result,
    SourceId, parse_pratt_symbol as parse_symbol,
};

/// Precedence-climbing parser for the Algol surface: tokenizes infix source and
/// parses it into [`LocatedExprTree`] forms using operator binding powers from
/// its [`PrattTable`].
pub struct PrattParser {
    pub(crate) operators: PrattTable,
}

impl PrattParser {
    /// Creates a parser driven by the given operator table. Use
    /// [`crate::default_pratt_table`] for the standard arithmetic operators.
    pub fn new(operators: PrattTable) -> Self {
        Self { operators }
    }

    /// Parses `source` into a located expression tree under a default decode
    /// budget. `source_id` names the input for origin tracking.
    pub fn parse_text_tree(
        &self,
        codec: sim_kernel::CodecId,
        source_id: impl Into<String>,
        source: &str,
    ) -> Result<LocatedExprTree> {
        let mut budget = DecodeBudget::new(DecodeLimits::default());
        self.parse_text_tree_with_budget(codec, source_id, source, &mut budget)
    }

    /// Parses `source` into a located expression tree under an explicit
    /// `budget`, erroring if any tokens remain after a complete expression.
    pub fn parse_text_tree_with_budget(
        &self,
        codec: sim_kernel::CodecId,
        source_id: impl Into<String>,
        source: &str,
        budget: &mut DecodeBudget,
    ) -> Result<LocatedExprTree> {
        let tokens = tokenize_algol_spanned_with_budget(source, budget)?;
        budget.check_tokens(codec, tokens.len())?;
        let mut cx = ParseCx::new(tokens);
        let expr = self.parse_expr_tree(
            &mut cx,
            codec,
            &SourceId(source_id.into()),
            source,
            0,
            budget,
            0,
        )?;
        if !cx.is_empty() {
            return Err(Error::Eval("trailing algol tokens".to_owned()));
        }
        Ok(expr)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_expr_tree(
        &self,
        cx: &mut ParseCx,
        codec: sim_kernel::CodecId,
        source_id: &SourceId,
        source: &str,
        min_bp: u16,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        // Charge depth/node budget BEFORE recursing into nud/led so that
        // unbalanced prefix-operator or open-paren chains (`- - - ...` or
        // `((((...`) fail closed on the depth limit instead of overflowing the
        // native stack: those paths recurse through `parse_expr_tree` without
        // reaching a leaf arm, where the budget was previously the only check.
        budget.enter_node(codec, depth)?;
        let mut left = self.parse_nud_tree(cx, codec, source_id, source, budget, depth)?;

        loop {
            if matches!(
                cx.peek(),
                Some(SpannedToken {
                    token: Token::OpenParen,
                    ..
                })
            ) {
                if 110 < min_bp {
                    break;
                }
                let open = cx.next_required()?;
                left = self.parse_call_tree(
                    cx, codec, source_id, source, left, open.start, budget, depth,
                )?;
                continue;
            }

            let Some(token) = cx.peek().cloned() else {
                break;
            };

            let Some(op) = self.operators.lookup_led(&token.token) else {
                break;
            };

            if op.fixity == Fixity::Postfix {
                if op.left_bp < min_bp {
                    break;
                }
                cx.advance();
                left = self.parse_postfix_tree(
                    codec, source_id, source, left, op, token.end, budget, depth,
                )?;
                continue;
            }

            if op.left_bp < min_bp {
                break;
            }

            cx.advance();
            left = self.parse_led_tree(cx, codec, source_id, source, left, op, budget, depth)?;
        }

        Ok(left)
    }

    fn parse_nud_tree(
        &self,
        cx: &mut ParseCx,
        codec: sim_kernel::CodecId,
        source_id: &SourceId,
        source: &str,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        let token = cx.next_required()?;

        if let Some(op) = self.operators.lookup_nud(&token.token) {
            let right =
                self.parse_expr_tree(cx, codec, source_id, source, op.right_bp, budget, depth + 1)?;
            return self.build_prefix_tree(
                codec,
                source_id,
                source,
                op,
                token.start,
                right,
                budget,
                depth,
            );
        }

        match token.token {
            Token::Ident(name) => {
                budget.enter_node(codec, depth)?;
                let expr = match name.as_str() {
                    "nil" => Expr::Nil,
                    "true" => Expr::Bool(true),
                    "false" => Expr::Bool(false),
                    _ => Expr::Symbol(parse_symbol(&name)),
                };
                Ok(LocatedExprTree::without_children(
                    expr,
                    Some(tree_origin(
                        codec,
                        source_id.clone(),
                        source,
                        token.start,
                        token.end,
                        token.leading_trivia.clone(),
                    )),
                ))
            }
            Token::Number(number) => {
                budget.enter_node(codec, depth)?;
                Ok(LocatedExprTree::without_children(
                    raw_number_expr(number),
                    Some(tree_origin(
                        codec,
                        source_id.clone(),
                        source,
                        token.start,
                        token.end,
                        token.leading_trivia.clone(),
                    )),
                ))
            }
            Token::String(value) => {
                budget.enter_node(codec, depth)?;
                budget.check_string_bytes(codec, value.len())?;
                Ok(LocatedExprTree::without_children(
                    Expr::String(value),
                    Some(tree_origin(
                        codec,
                        source_id.clone(),
                        source,
                        token.start,
                        token.end,
                        token.leading_trivia.clone(),
                    )),
                ))
            }
            Token::OpenParen => {
                let mut expr =
                    self.parse_expr_tree(cx, codec, source_id, source, 0, budget, depth + 1)?;
                let close = cx.next_required()?;
                if close.token != Token::CloseParen {
                    return Err(Error::Eval(format!(
                        "expected ')' in algol input, found {:?}",
                        close
                    )));
                }
                extend_tree_trivia(&mut expr, close.leading_trivia.clone());
                Ok(with_origin_span(
                    expr,
                    tree_origin(codec, source_id.clone(), source, token.start, close.end, {
                        let mut trivia = token.leading_trivia.clone();
                        trivia.extend(close.leading_trivia.clone());
                        trivia
                    }),
                ))
            }
            other => Err(Error::Eval(format!(
                "unexpected algol token in nud {:?}",
                other
            ))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_led_tree(
        &self,
        cx: &mut ParseCx,
        codec: sim_kernel::CodecId,
        source_id: &SourceId,
        source: &str,
        left: LocatedExprTree,
        op: PrattOperator,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        match op.fixity {
            Fixity::InfixLeft | Fixity::InfixRight => {
                let right = self.parse_expr_tree(
                    cx,
                    codec,
                    source_id,
                    source,
                    op.right_bp,
                    budget,
                    depth + 1,
                )?;
                self.build_infix_tree(codec, source_id, source, op, left, right, budget, depth)
            }
            _ => Err(Error::Eval("operator cannot be used here".to_owned())),
        }
    }
}
