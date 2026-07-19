use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{
    CodecId, Error, Expr, LocatedExprTree, PrattOperator, PrattToken, Result, SourceId, Symbol,
};

use crate::LUA_CODEC_ID;
use crate::ast::{LuaBinOp, LuaExpr, LuaField, LuaUnOp};
use crate::lex::{LuaToken, LuaTokenKind, tokenize_lua_with_budget};
use crate::pratt::{lua_pratt_parser, lua_pratt_table};

/// Parses one Lua expression under default decode limits.
pub fn parse_lua_expr(source: &str) -> Result<LuaExpr> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    parse_lua_expr_with_budget(LUA_CODEC_ID, source, &mut budget)
}

/// Parses one Lua expression under an explicit decode budget.
pub fn parse_lua_expr_with_budget(
    codec: CodecId,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<LuaExpr> {
    let tokens = tokenize_lua_with_budget(codec, source, budget)?;
    let mut parser = LuaExprParser::new(codec, tokens);
    let expr = parser.parse_expr_bp(0, budget, 0)?;
    parser.expect_end()?;
    Ok(expr)
}

/// Parses one Lua expression into a shared located `Expr` tree.
///
/// This entry point is for the Pratt-compatible subset of Lua expressions:
/// literals, names, calls, grouping, and prefix/infix operators. Lua table and
/// indexing forms are represented by [`parse_lua_expr`].
pub fn parse_lua_expr_tree(source_id: impl Into<String>, source: &str) -> Result<LocatedExprTree> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    parse_lua_expr_tree_with_budget(LUA_CODEC_ID, source_id, source, &mut budget)
}

/// Parses one Pratt-compatible Lua expression into a shared located tree under
/// an explicit decode budget.
pub fn parse_lua_expr_tree_with_budget(
    codec: CodecId,
    source_id: impl Into<String>,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<LocatedExprTree> {
    budget.check_input_bytes(codec, source.len())?;
    lua_pratt_parser().parse_tree_with_source_and_budget(
        codec,
        SourceId(source_id.into()),
        source,
        budget,
    )
}

struct LuaExprParser {
    codec: CodecId,
    tokens: Vec<LuaToken>,
    index: usize,
    table: sim_kernel::PrattTable,
}

impl LuaExprParser {
    fn new(codec: CodecId, tokens: Vec<LuaToken>) -> Self {
        Self {
            codec,
            tokens,
            index: 0,
            table: lua_pratt_table(),
        }
    }

    fn parse_expr_bp(
        &mut self,
        min_bp: u16,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LuaExpr> {
        budget.enter_node(self.codec, depth)?;
        let mut left = self.parse_prefix_or_primary(budget, depth + 1)?;

        loop {
            if self.consume_kind(&LuaTokenKind::OpenParen) {
                left = self.finish_call(left, budget, depth + 1)?;
                continue;
            }
            if self.consume_kind(&LuaTokenKind::Dot) {
                let name = self.expect_identifier("field name after '.'")?;
                budget.enter_node(self.codec, depth)?;
                left = LuaExpr::Index {
                    obj: Box::new(left),
                    key: Box::new(LuaExpr::Str(name)),
                };
                continue;
            }
            if self.consume_kind(&LuaTokenKind::OpenBracket) {
                let key = self.parse_expr_bp(0, budget, depth + 1)?;
                self.expect_kind(&LuaTokenKind::CloseBracket, "']' after index")?;
                budget.enter_node(self.codec, depth)?;
                left = LuaExpr::Index {
                    obj: Box::new(left),
                    key: Box::new(key),
                };
                continue;
            }
            if self.consume_kind(&LuaTokenKind::Colon) {
                let name = self.expect_identifier("method name after ':'")?;
                self.expect_kind(&LuaTokenKind::OpenParen, "'(' after method name")?;
                let args = self.parse_call_args(budget, depth + 1)?;
                budget.enter_node(self.codec, depth)?;
                left = LuaExpr::Method {
                    recv: Box::new(left),
                    name: Symbol::new(name),
                    args,
                };
                continue;
            }

            let Some((op, operator)) = self.peek_infix_operator() else {
                break;
            };
            if operator.left_bp < min_bp {
                break;
            }
            self.advance();
            let right = self.parse_expr_bp(operator.right_bp, budget, depth + 1)?;
            budget.enter_node(self.codec, depth)?;
            left = LuaExpr::Binary {
                op,
                lhs: Box::new(left),
                rhs: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_prefix_or_primary(
        &mut self,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LuaExpr> {
        if let Some((op, operator)) = self.peek_prefix_operator() {
            self.advance();
            let rhs = self.parse_expr_bp(operator.right_bp, budget, depth + 1)?;
            return Ok(LuaExpr::Unary {
                op,
                rhs: Box::new(rhs),
            });
        }

        let token = self.advance_required()?;
        match token.kind {
            LuaTokenKind::Identifier(text) => Ok(match text.as_str() {
                "nil" => LuaExpr::Nil,
                "true" => LuaExpr::True,
                "false" => LuaExpr::False,
                _ => LuaExpr::Name(Symbol::new(text)),
            }),
            LuaTokenKind::Number(raw) => Ok(LuaExpr::Number(raw)),
            LuaTokenKind::String(text) => Ok(LuaExpr::Str(text)),
            LuaTokenKind::Vararg => Ok(LuaExpr::Vararg),
            LuaTokenKind::OpenParen => {
                let expr = self.parse_expr_bp(0, budget, depth + 1)?;
                self.expect_kind(&LuaTokenKind::CloseParen, "')' after grouped expression")?;
                Ok(expr)
            }
            LuaTokenKind::OpenBrace => self.parse_table(budget, depth + 1),
            other => Err(Error::Eval(format!(
                "expected lua expression, found {other:?}"
            ))),
        }
    }

    fn parse_table(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaExpr> {
        let mut fields = Vec::new();
        if self.consume_kind(&LuaTokenKind::CloseBrace) {
            return Ok(LuaExpr::Table(fields));
        }

        loop {
            budget.check_collection_len(self.codec, fields.len() + 1)?;
            let field = if self.consume_kind(&LuaTokenKind::OpenBracket) {
                let key = self.parse_expr_bp(0, budget, depth + 1)?;
                self.expect_kind(&LuaTokenKind::CloseBracket, "']' after table key")?;
                self.expect_kind(&LuaTokenKind::Equal, "'=' after table key")?;
                let value = self.parse_expr_bp(0, budget, depth + 1)?;
                LuaField::Keyed { key, value }
            } else if let Some(name) = self.peek_named_field() {
                self.advance();
                self.expect_kind(&LuaTokenKind::Equal, "'=' after table field name")?;
                let value = self.parse_expr_bp(0, budget, depth + 1)?;
                LuaField::Named {
                    key: Symbol::new(name),
                    value,
                }
            } else {
                LuaField::Positional(self.parse_expr_bp(0, budget, depth + 1)?)
            };
            fields.push(field);

            if self.consume_kind(&LuaTokenKind::CloseBrace) {
                break;
            }
            if self.consume_kind(&LuaTokenKind::Comma) || self.consume_kind(&LuaTokenKind::Semi) {
                if self.consume_kind(&LuaTokenKind::CloseBrace) {
                    break;
                }
                continue;
            }
            self.expect_kind(&LuaTokenKind::CloseBrace, "'}' or field separator")?;
            break;
        }

        Ok(LuaExpr::Table(fields))
    }

    fn finish_call(
        &mut self,
        callee: LuaExpr,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LuaExpr> {
        let args = self.parse_call_args(budget, depth + 1)?;
        budget.enter_node(self.codec, depth)?;
        Ok(LuaExpr::Call {
            callee: Box::new(callee),
            args,
        })
    }

    fn parse_call_args(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<Vec<LuaExpr>> {
        let mut args = Vec::new();
        if self.consume_kind(&LuaTokenKind::CloseParen) {
            return Ok(args);
        }
        loop {
            budget.check_collection_len(self.codec, args.len() + 1)?;
            args.push(self.parse_expr_bp(0, budget, depth + 1)?);
            if self.consume_kind(&LuaTokenKind::CloseParen) {
                break;
            }
            self.expect_kind(&LuaTokenKind::Comma, "',' or ')' in argument list")?;
        }
        Ok(args)
    }

    fn peek_prefix_operator(&self) -> Option<(LuaUnOp, PrattOperator)> {
        let token = self.peek()?.kind.pratt_lookup_token()?;
        let operator = self.table.lookup_nud(&token)?;
        let op = LuaUnOp::from_symbol(operator.symbol.to_string().as_str())?;
        Some((op, operator))
    }

    fn peek_infix_operator(&self) -> Option<(LuaBinOp, PrattOperator)> {
        let token = self.peek()?.kind.pratt_lookup_token()?;
        let operator = self.table.lookup_led(&token)?;
        let op = LuaBinOp::from_symbol(operator.symbol.to_string().as_str())?;
        Some((op, operator))
    }

    fn peek_named_field(&self) -> Option<String> {
        let LuaTokenKind::Identifier(name) = &self.peek()?.kind else {
            return None;
        };
        matches!(
            self.tokens.get(self.index + 1).map(|token| &token.kind),
            Some(LuaTokenKind::Equal)
        )
        .then(|| name.clone())
    }

    fn expect_identifier(&mut self, label: &str) -> Result<String> {
        let token = self.advance_required()?;
        match token.kind {
            LuaTokenKind::Identifier(name) => Ok(name),
            other => Err(Error::Eval(format!("expected {label}, found {other:?}"))),
        }
    }

    fn expect_end(&self) -> Result<()> {
        if let Some(token) = self.peek() {
            return Err(Error::Eval(format!("trailing lua token {:?}", token.kind)));
        }
        Ok(())
    }

    fn expect_kind(&mut self, expected: &LuaTokenKind, label: &str) -> Result<()> {
        if self.consume_kind(expected) {
            return Ok(());
        }
        Err(Error::Eval(format!("expected {label}")))
    }

    fn consume_kind(&mut self, expected: &LuaTokenKind) -> bool {
        if self.peek().is_some_and(|token| token.kind == *expected) {
            self.index += 1;
            return true;
        }
        false
    }

    fn advance_required(&mut self) -> Result<LuaToken> {
        self.advance()
            .ok_or_else(|| Error::Eval("unexpected end of lua input".to_owned()))
    }

    fn advance(&mut self) -> Option<LuaToken> {
        let token = self.tokens.get(self.index).cloned()?;
        self.index += 1;
        Some(token)
    }

    fn peek(&self) -> Option<&LuaToken> {
        self.tokens.get(self.index)
    }
}

fn _assert_pratt_token_send_sync(_: PrattToken, _: Expr) {}
