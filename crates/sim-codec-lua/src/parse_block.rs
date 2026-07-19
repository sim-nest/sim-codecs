use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{CodecId, Error, Result, Symbol};

use crate::LUA_CODEC_ID;
use crate::ast::{
    LuaBinding, LuaBlock, LuaFuncBody, LuaFunctionName, LuaIfArm, LuaLocalAttr, LuaStmt,
};
use crate::lex::{LuaTokenKind, tokenize_lua_with_budget};
use crate::parse_expr::LuaExprParser;

/// Parses a complete Lua chunk under default decode limits.
pub fn parse_lua_chunk(source: &str) -> Result<LuaBlock> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    parse_lua_chunk_with_budget(LUA_CODEC_ID, source, &mut budget)
}

/// Parses a complete Lua chunk under an explicit decode budget.
pub fn parse_lua_chunk_with_budget(
    codec: CodecId,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<LuaBlock> {
    let tokens = tokenize_lua_with_budget(codec, source, budget)?;
    let mut expr = LuaExprParser::new(codec, tokens);
    let block = LuaBlockParser::new(&mut expr).parse_block_until(&[], budget, 0)?;
    expr.expect_end()?;
    Ok(block)
}

pub(crate) fn parse_lua_function_body_from_parser(
    parser: &mut LuaExprParser,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<LuaFuncBody> {
    LuaBlockParser::new(parser).parse_function_body(budget, depth)
}

struct LuaBlockParser<'a> {
    expr: &'a mut LuaExprParser,
}

impl<'a> LuaBlockParser<'a> {
    fn new(expr: &'a mut LuaExprParser) -> Self {
        Self { expr }
    }

    fn parse_block_until(
        &mut self,
        stop_keywords: &[&str],
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LuaBlock> {
        let mut statements = Vec::new();
        while !self.expr.is_empty() && !self.expr.at_keyword(stop_keywords) {
            if self.expr.consume_kind(&LuaTokenKind::Semi) {
                continue;
            }
            budget.check_collection_len(self.expr.codec(), statements.len() + 1)?;
            statements.push(self.parse_stmt(budget, depth + 1)?);
        }
        Ok(LuaBlock { statements })
    }

    fn parse_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        if self.expr.consume_keyword("local") {
            return self.parse_local_stmt(budget, depth + 1);
        }
        if self.expr.consume_keyword("if") {
            return self.parse_if_stmt(budget, depth + 1);
        }
        if self.expr.consume_keyword("while") {
            return self.parse_while_stmt(budget, depth + 1);
        }
        if self.expr.consume_keyword("repeat") {
            return self.parse_repeat_stmt(budget, depth + 1);
        }
        if self.expr.consume_keyword("for") {
            return self.parse_for_stmt(budget, depth + 1);
        }
        if self.expr.consume_keyword("function") {
            return self.parse_function_stmt(budget, depth + 1);
        }
        if self.expr.consume_keyword("return") {
            return self.parse_return_stmt(budget, depth + 1);
        }
        if self.expr.consume_keyword("break") {
            return Ok(LuaStmt::Break);
        }
        if self.expr.consume_keyword("do") {
            let block = self.parse_block_until(&["end"], budget, depth + 1)?;
            self.expr.expect_keyword("end")?;
            return Ok(LuaStmt::Do(block));
        }
        if self.expr.consume_keyword("goto") {
            let name = self.expr.expect_identifier("label after goto")?;
            return Ok(LuaStmt::Goto(Symbol::new(name)));
        }
        if self.expr.consume_kind(&LuaTokenKind::DoubleColon) {
            let name = self.expr.expect_identifier("label name")?;
            self.expr
                .expect_kind(&LuaTokenKind::DoubleColon, "'::' after label name")?;
            return Ok(LuaStmt::Label(Symbol::new(name)));
        }
        self.parse_assignment_or_expr_stmt(budget, depth + 1)
    }

    fn parse_local_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        if self.expr.consume_keyword("function") {
            let name = Symbol::new(self.expr.expect_identifier("local function name")?);
            let body = self.parse_function_body(budget, depth + 1)?;
            return Ok(LuaStmt::LocalFunction { name, body });
        }

        let mut bindings = Vec::new();
        loop {
            budget.check_collection_len(self.expr.codec(), bindings.len() + 1)?;
            let name = Symbol::new(self.expr.expect_identifier("local name")?);
            let attr = self.parse_local_attr()?;
            bindings.push(LuaBinding { name, attr });
            if !self.expr.consume_kind(&LuaTokenKind::Comma) {
                break;
            }
        }

        let values = if self.expr.consume_kind(&LuaTokenKind::Equal) {
            self.parse_expr_list(budget, depth + 1)?
        } else {
            Vec::new()
        };
        Ok(LuaStmt::Local { bindings, values })
    }

    fn parse_local_attr(&mut self) -> Result<Option<LuaLocalAttr>> {
        if !self.expr.consume_operator("<") {
            return Ok(None);
        }
        let attr = match self.expr.expect_identifier("local attribute")?.as_str() {
            "const" => LuaLocalAttr::Const,
            "close" => LuaLocalAttr::Close,
            other => {
                return Err(Error::Eval(format!(
                    "unsupported lua local attribute {other}"
                )));
            }
        };
        self.expr.expect_operator(">")?;
        Ok(Some(attr))
    }

    fn parse_if_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        let mut arms = Vec::new();
        let condition = self.expr.parse_expr(budget, depth + 1)?;
        self.expr.expect_keyword("then")?;
        let block = self.parse_block_until(&["elseif", "else", "end"], budget, depth + 1)?;
        arms.push(LuaIfArm { condition, block });

        while self.expr.consume_keyword("elseif") {
            let condition = self.expr.parse_expr(budget, depth + 1)?;
            self.expr.expect_keyword("then")?;
            let block = self.parse_block_until(&["elseif", "else", "end"], budget, depth + 1)?;
            arms.push(LuaIfArm { condition, block });
        }

        let else_block = if self.expr.consume_keyword("else") {
            Some(self.parse_block_until(&["end"], budget, depth + 1)?)
        } else {
            None
        };
        self.expr.expect_keyword("end")?;
        Ok(LuaStmt::If { arms, else_block })
    }

    fn parse_while_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        let condition = self.expr.parse_expr(budget, depth + 1)?;
        self.expr.expect_keyword("do")?;
        let block = self.parse_block_until(&["end"], budget, depth + 1)?;
        self.expr.expect_keyword("end")?;
        Ok(LuaStmt::While { condition, block })
    }

    fn parse_repeat_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        let block = self.parse_block_until(&["until"], budget, depth + 1)?;
        self.expr.expect_keyword("until")?;
        let condition = self.expr.parse_expr(budget, depth + 1)?;
        Ok(LuaStmt::Repeat { block, condition })
    }

    fn parse_for_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        let first = Symbol::new(self.expr.expect_identifier("for variable")?);
        if self.expr.consume_kind(&LuaTokenKind::Equal) {
            let start = self.expr.parse_expr(budget, depth + 1)?;
            self.expr
                .expect_kind(&LuaTokenKind::Comma, "',' after for start")?;
            let limit = self.expr.parse_expr(budget, depth + 1)?;
            let step = if self.expr.consume_kind(&LuaTokenKind::Comma) {
                Some(self.expr.parse_expr(budget, depth + 1)?)
            } else {
                None
            };
            self.expr.expect_keyword("do")?;
            let block = self.parse_block_until(&["end"], budget, depth + 1)?;
            self.expr.expect_keyword("end")?;
            return Ok(LuaStmt::NumericFor {
                name: first,
                start,
                limit,
                step,
                block,
            });
        }

        let mut names = vec![first];
        while self.expr.consume_kind(&LuaTokenKind::Comma) {
            names.push(Symbol::new(self.expr.expect_identifier("for variable")?));
        }
        self.expr.expect_keyword("in")?;
        let iter = self.parse_expr_list(budget, depth + 1)?;
        self.expr.expect_keyword("do")?;
        let block = self.parse_block_until(&["end"], budget, depth + 1)?;
        self.expr.expect_keyword("end")?;
        Ok(LuaStmt::GenericFor { names, iter, block })
    }

    fn parse_function_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        let name = self.parse_function_name()?;
        let body = self.parse_function_body(budget, depth + 1)?;
        Ok(LuaStmt::Function { name, body })
    }

    fn parse_function_name(&mut self) -> Result<LuaFunctionName> {
        let base = Symbol::new(self.expr.expect_identifier("function name")?);
        let mut fields = Vec::new();
        while self.expr.consume_kind(&LuaTokenKind::Dot) {
            fields.push(Symbol::new(
                self.expr.expect_identifier("function field name")?,
            ));
        }
        let method = if self.expr.consume_kind(&LuaTokenKind::Colon) {
            Some(Symbol::new(
                self.expr.expect_identifier("function method name")?,
            ))
        } else {
            None
        };
        Ok(LuaFunctionName {
            base,
            fields,
            method,
        })
    }

    fn parse_function_body(
        &mut self,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LuaFuncBody> {
        self.expr
            .expect_kind(&LuaTokenKind::OpenParen, "'(' before parameters")?;
        let (params, vararg) = self.parse_params()?;
        self.expr
            .expect_kind(&LuaTokenKind::CloseParen, "')' after parameters")?;
        let block = self.parse_block_until(&["end"], budget, depth + 1)?;
        self.expr.expect_keyword("end")?;
        Ok(LuaFuncBody {
            params,
            vararg,
            block,
        })
    }

    fn parse_params(&mut self) -> Result<(Vec<Symbol>, bool)> {
        let mut params = Vec::new();
        let mut vararg = false;
        if matches!(
            self.expr.peek().map(|token| &token.kind),
            Some(LuaTokenKind::CloseParen)
        ) {
            return Ok((params, vararg));
        }
        loop {
            if self.expr.consume_kind(&LuaTokenKind::Vararg) {
                vararg = true;
                break;
            }
            params.push(Symbol::new(self.expr.expect_identifier("parameter name")?));
            if !self.expr.consume_kind(&LuaTokenKind::Comma) {
                break;
            }
        }
        Ok((params, vararg))
    }

    fn parse_return_stmt(&mut self, budget: &mut DecodeBudget, depth: usize) -> Result<LuaStmt> {
        let values = if self.at_statement_end() {
            Vec::new()
        } else {
            self.parse_expr_list(budget, depth + 1)?
        };
        let _ = self.expr.consume_kind(&LuaTokenKind::Semi);
        Ok(LuaStmt::Return(values))
    }

    fn parse_assignment_or_expr_stmt(
        &mut self,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LuaStmt> {
        let first = self.expr.parse_expr(budget, depth + 1)?;
        if self.expr.consume_kind(&LuaTokenKind::Equal)
            || matches!(
                self.expr.peek().map(|token| &token.kind),
                Some(LuaTokenKind::Comma)
            )
        {
            let mut targets = vec![first];
            while self.expr.consume_kind(&LuaTokenKind::Comma) {
                targets.push(self.expr.parse_expr(budget, depth + 1)?);
            }
            if targets.len() > 1 {
                self.expr
                    .expect_kind(&LuaTokenKind::Equal, "'=' after assignment targets")?;
            }
            let values = self.parse_expr_list(budget, depth + 1)?;
            return Ok(LuaStmt::Assign { targets, values });
        }
        Ok(LuaStmt::Expr(first))
    }

    fn parse_expr_list(
        &mut self,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<Vec<crate::LuaExpr>> {
        let mut values = Vec::new();
        loop {
            budget.check_collection_len(self.expr.codec(), values.len() + 1)?;
            values.push(self.expr.parse_expr(budget, depth + 1)?);
            if !self.expr.consume_kind(&LuaTokenKind::Comma) {
                break;
            }
        }
        Ok(values)
    }

    fn at_statement_end(&self) -> bool {
        self.expr.is_empty()
            || self.expr.at_keyword(&["end", "else", "elseif", "until"])
            || matches!(
                self.expr.peek().map(|token| &token.kind),
                Some(LuaTokenKind::Semi)
            )
    }
}
