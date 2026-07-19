use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{
    CodecId, Error, Expr, Fixity, LocatedExprTree, Origin, PrattOperator, PrattTable,
    PrattToken as Token, Result, SourceId, Span, Symbol, Trivia,
    parse_pratt_symbol as parse_symbol,
};

use crate::{PrattTokenSource, SpannedPrattToken};

/// Returns the raw-number tag consumed by Algol number-domain lowering.
pub fn raw_number_tag() -> Symbol {
    Symbol::qualified("codec", "algol-number-literal")
}

/// Builds the raw-number expression emitted for Pratt numeric literal tokens.
pub fn raw_number_expr(raw: String) -> Expr {
    Expr::Extension {
        tag: raw_number_tag(),
        payload: Box::new(Expr::String(raw)),
    }
}

/// Language-neutral Pratt driver: an operator table plus any token source.
pub struct PrattCodecParser<S> {
    operators: PrattTable,
    token_source: S,
    surface_name: &'static str,
}

impl<S: PrattTokenSource> PrattCodecParser<S> {
    /// Creates a parser driven by `operators` and `token_source`.
    pub fn new(operators: PrattTable, token_source: S) -> Self {
        Self {
            operators,
            token_source,
            surface_name: "pratt",
        }
    }

    /// Sets the surface name used in parse error messages.
    pub fn with_surface_name(mut self, surface_name: &'static str) -> Self {
        self.surface_name = surface_name;
        self
    }

    /// Returns the parser's operator table.
    pub fn operators(&self) -> &PrattTable {
        &self.operators
    }

    /// Returns the parser's token source.
    pub fn token_source(&self) -> &S {
        &self.token_source
    }

    /// Parses `source` into a located expression tree under a default budget.
    pub fn parse_tree(&self, codec: CodecId, source: &str) -> Result<LocatedExprTree> {
        let mut budget = DecodeBudget::new(DecodeLimits::default());
        self.parse_tree_with_budget(codec, source, &mut budget)
    }

    /// Parses `source` into a located expression tree under an explicit budget.
    pub fn parse_tree_with_budget(
        &self,
        codec: CodecId,
        source: &str,
        budget: &mut DecodeBudget,
    ) -> Result<LocatedExprTree> {
        self.parse_tree_with_source_and_budget(
            codec,
            SourceId(format!("<{}>", self.surface_name)),
            source,
            budget,
        )
    }

    /// Parses `source` into a located tree with the caller's source id.
    pub fn parse_tree_with_source(
        &self,
        codec: CodecId,
        source_id: impl Into<String>,
        source: &str,
    ) -> Result<LocatedExprTree> {
        let mut budget = DecodeBudget::new(DecodeLimits::default());
        self.parse_tree_with_source_and_budget(
            codec,
            SourceId(source_id.into()),
            source,
            &mut budget,
        )
    }

    /// Parses `source` into a located tree with the caller's source id and budget.
    pub fn parse_tree_with_source_and_budget(
        &self,
        codec: CodecId,
        source_id: SourceId,
        source: &str,
        budget: &mut DecodeBudget,
    ) -> Result<LocatedExprTree> {
        let tokens = self.token_source.tokenize_pratt(codec, source, budget)?;
        budget.check_tokens(codec, tokens.len())?;
        let mut cx = ParseCx::new(tokens, self.surface_name);
        let expr = self.parse_expr_tree(&mut cx, codec, &source_id, source, 0, budget, 0)?;
        if !cx.is_empty() {
            return Err(Error::Eval(format!(
                "trailing {} tokens",
                self.surface_name
            )));
        }
        Ok(expr)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_expr_tree(
        &self,
        cx: &mut ParseCx,
        codec: CodecId,
        source_id: &SourceId,
        source: &str,
        min_bp: u16,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        budget.enter_node(codec, depth)?;
        let mut left = self.parse_nud_tree(cx, codec, source_id, source, budget, depth)?;

        loop {
            if matches!(
                cx.peek(),
                Some(SpannedPrattToken {
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
        codec: CodecId,
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
                        token.leading_trivia,
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
                        token.leading_trivia,
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
                        token.leading_trivia,
                    )),
                ))
            }
            Token::OpenParen => {
                let mut expr =
                    self.parse_expr_tree(cx, codec, source_id, source, 0, budget, depth + 1)?;
                let close = cx.next_required()?;
                if close.token != Token::CloseParen {
                    return Err(Error::Eval(format!(
                        "expected ')' in {} input, found {:?}",
                        self.surface_name, close
                    )));
                }
                extend_tree_trivia(&mut expr, close.leading_trivia.clone());
                Ok(with_origin_span(
                    expr,
                    tree_origin(codec, source_id.clone(), source, token.start, close.end, {
                        let mut trivia = token.leading_trivia;
                        trivia.extend(close.leading_trivia);
                        trivia
                    }),
                ))
            }
            other => Err(Error::Eval(format!(
                "unexpected {} token in nud {:?}",
                self.surface_name, other
            ))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_led_tree(
        &self,
        cx: &mut ParseCx,
        codec: CodecId,
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

pub(super) struct ParseCx {
    tokens: Vec<SpannedPrattToken>,
    index: usize,
    surface_name: &'static str,
}

impl ParseCx {
    pub(super) fn new(tokens: Vec<SpannedPrattToken>, surface_name: &'static str) -> Self {
        Self {
            tokens,
            index: 0,
            surface_name,
        }
    }

    pub(super) fn peek(&self) -> Option<&SpannedPrattToken> {
        self.tokens.get(self.index)
    }

    pub(super) fn advance(&mut self) -> Option<SpannedPrattToken> {
        let token = self.tokens.get(self.index).cloned()?;
        self.index += 1;
        Some(token)
    }

    pub(super) fn next_required(&mut self) -> Result<SpannedPrattToken> {
        self.advance()
            .ok_or_else(|| Error::Eval(format!("unexpected end of {} input", self.surface_name)))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.index >= self.tokens.len()
    }
}

pub(super) fn with_origin_span(mut tree: LocatedExprTree, origin: Origin) -> LocatedExprTree {
    tree.origin = Some(origin);
    tree
}

pub(super) fn extend_tree_trivia(tree: &mut LocatedExprTree, trivia: Vec<Trivia>) {
    if trivia.is_empty() {
        return;
    }
    if let Some(origin) = &mut tree.origin {
        origin.trivia.extend(trivia);
    }
}

pub(super) fn tree_origin(
    codec: CodecId,
    source: SourceId,
    _raw: &str,
    start: usize,
    end: usize,
    trivia: Vec<Trivia>,
) -> Origin {
    Origin {
        codec,
        source,
        span: Span { start, end },
        trivia,
    }
}
