//! Token-tree reader for the Lisp codec: consumes a lexed token stream and
//! builds a located expression tree, resolving nesting, quote forms, and trivia.

use proc_macro2::Delimiter;
use sim_codec::ReadCx;
use sim_kernel::{
    Error, Expr, LocatedExprTree, Origin, QuoteMode, Result, SourceId, Span, Symbol, Trivia, Value,
    read_construct_capability, read_eval_capability,
};

use super::forms::{
    decode_data_expr, lower_eval_surface, may_be_number_literal, parse_symbol, read_escape_form,
    read_explicit_quote,
};
use super::lex::{LispToken, LispTokenKind, extend_tree_trivia, matches_closer};

pub(crate) struct LispTreeReader<'a, 'cx, 'b> {
    cx: &'a mut ReadCx<'cx>,
    budget: &'b mut sim_codec::DecodeBudget,
    source_id: SourceId,
    tokens: Vec<LispToken>,
    index: usize,
}

impl<'a, 'cx, 'b> LispTreeReader<'a, 'cx, 'b> {
    pub(crate) fn new(
        cx: &'a mut ReadCx<'cx>,
        source_id: SourceId,
        _source: &'a str,
        tokens: Vec<LispToken>,
        budget: &'b mut sim_codec::DecodeBudget,
    ) -> Self {
        Self {
            cx,
            budget,
            source_id,
            tokens,
            index: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn peek(&self) -> Option<&LispToken> {
        self.tokens.get(self.index)
    }

    fn next(&mut self) -> Result<LispToken> {
        let token = self
            .tokens
            .get(self.index)
            .cloned()
            .ok_or(Error::CodecError {
                codec: self.cx.codec,
                message: "unexpected end of input".to_owned(),
            })?;
        self.index += 1;
        Ok(token)
    }

    pub(crate) fn read_one(&mut self, depth: usize) -> Result<LocatedExprTree> {
        let token = self.next()?;
        self.read_token(token, depth)
    }

    fn read_token(&mut self, token: LispToken, depth: usize) -> Result<LocatedExprTree> {
        match token.kind {
            LispTokenKind::OpenParen => self.read_group(
                Delimiter::Parenthesis,
                token.start,
                token.leading_trivia,
                depth,
            ),
            LispTokenKind::OpenBracket => {
                self.read_group(Delimiter::Bracket, token.start, token.leading_trivia, depth)
            }
            LispTokenKind::OpenBrace => {
                self.read_group(Delimiter::Brace, token.start, token.leading_trivia, depth)
            }
            LispTokenKind::Quote => {
                self.budget.enter_node(self.cx.codec, depth)?;
                let expr = self.read_one(depth + 1)?;
                let end = expr
                    .origin
                    .as_ref()
                    .map(|origin| origin.span.end)
                    .unwrap_or(token.end);
                let origin =
                    self.origin_with_trivia(token.start, end, token.leading_trivia.clone())?;
                Ok(LocatedExprTree {
                    expr: Expr::Quote {
                        mode: QuoteMode::Quote,
                        expr: Box::new(expr.expr.clone()),
                    },
                    origin: Some(origin),
                    children: vec![expr],
                })
            }
            LispTokenKind::Dispatch => self.read_dispatch(token.start, depth),
            LispTokenKind::Atom(text) => self.atom_expr(
                text,
                token.start,
                token.end,
                token.leading_trivia.clone(),
                depth,
            ),
            LispTokenKind::String(text) => {
                self.budget.enter_node(self.cx.codec, depth)?;
                self.budget.check_string_bytes(self.cx.codec, text.len())?;
                Ok(LocatedExprTree::without_children(
                    Expr::String(text),
                    Some(self.origin_with_trivia(
                        token.start,
                        token.end,
                        token.leading_trivia.clone(),
                    )?),
                ))
            }
            LispTokenKind::Bytes(bytes) => {
                self.budget.enter_node(self.cx.codec, depth)?;
                self.budget.check_blob_bytes(self.cx.codec, bytes.len())?;
                Ok(LocatedExprTree::without_children(
                    Expr::Bytes(bytes),
                    Some(self.origin_with_trivia(
                        token.start,
                        token.end,
                        token.leading_trivia.clone(),
                    )?),
                ))
            }
            other => Err(self.error(format!("unexpected token {other:?}"))),
        }
    }

    fn read_group(
        &mut self,
        delimiter: Delimiter,
        start: usize,
        leading_trivia: Vec<Trivia>,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        let mut items: Vec<LocatedExprTree> = Vec::new();
        loop {
            let Some(token) = self.peek().cloned() else {
                return Err(self.error("unexpected end of grouped input"));
            };
            if matches_closer(delimiter, &token.kind) {
                let close = self.next()?;
                let mut parent_trivia = leading_trivia;
                parent_trivia.extend(close.leading_trivia.clone());
                if let Some(last) = items.last_mut() {
                    extend_tree_trivia(last, close.leading_trivia.clone());
                }
                return self.finish_group(delimiter, start, close.end, parent_trivia, items, depth);
            }
            if let Some(last) = items.last_mut() {
                extend_tree_trivia(last, token.leading_trivia.clone());
            }
            self.budget
                .check_collection_len(self.cx.codec, items.len() + 1)?;
            items.push(self.read_one(depth + 1)?);
        }
    }

    fn finish_group(
        &mut self,
        delimiter: Delimiter,
        start: usize,
        end: usize,
        leading_trivia: Vec<Trivia>,
        items: Vec<LocatedExprTree>,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        self.budget.enter_node(self.cx.codec, depth)?;
        let expr_items = items
            .iter()
            .map(|item| item.expr.clone())
            .collect::<Vec<_>>();
        let expr = match delimiter {
            Delimiter::Parenthesis => {
                if let Some(quoted) = read_explicit_quote(&expr_items) {
                    quoted
                } else if let Some(escaped) = read_escape_form(&expr_items)? {
                    escaped
                } else {
                    Expr::List(expr_items)
                }
            }
            Delimiter::Bracket => Expr::Vector(expr_items),
            Delimiter::Brace | Delimiter::None => Expr::Block(expr_items),
        };
        Ok(LocatedExprTree {
            expr,
            origin: Some(self.origin_with_trivia(start, end, leading_trivia)?),
            children: items,
        })
    }

    fn atom_expr(
        &mut self,
        text: String,
        start: usize,
        end: usize,
        leading_trivia: Vec<Trivia>,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        self.budget.enter_node(self.cx.codec, depth)?;
        let expr = if text == "nil" {
            Expr::Nil
        } else if text == "true" {
            Expr::Bool(true)
        } else if text == "false" {
            Expr::Bool(false)
        } else if may_be_number_literal(&text) {
            self.cx
                .cx
                .parse_number_literal(&text)?
                .map(Expr::Number)
                .unwrap_or_else(|| Expr::Symbol(parse_symbol(&text)))
        } else {
            Expr::Symbol(parse_symbol(&text))
        };
        Ok(LocatedExprTree::without_children(
            expr,
            Some(self.origin_with_trivia(start, end, leading_trivia)?),
        ))
    }

    fn read_dispatch(&mut self, start: usize, depth: usize) -> Result<LocatedExprTree> {
        self.budget.enter_node(self.cx.codec, depth)?;
        let token = self.next()?;
        match token.kind {
            LispTokenKind::OpenParen => self.read_construct(start, depth + 1),
            LispTokenKind::Atom(name) if name == "eval" => {
                let next = self.next()?;
                if next.kind != LispTokenKind::OpenParen {
                    return Err(self.error("expected #eval(...)"));
                }
                self.read_eval(start, depth + 1)
            }
            LispTokenKind::Atom(name) if name == "." => {
                // `#.` is Common Lisp read-time eval; gate it on the same
                // capability as `#eval`/`#(...)`, which is otherwise theater
                // while `#.` evaluates untrusted input unguarded beside them.
                self.cx.read_policy.require(&read_eval_capability())?;
                let expr = self.read_one(depth + 1)?;
                let end = expr
                    .origin
                    .as_ref()
                    .map(|origin| origin.span.end)
                    .unwrap_or(token.end);
                let value = self.eval_read_expr(expr.expr.clone())?;
                Ok(LocatedExprTree {
                    expr: value,
                    origin: Some(self.origin(start, end)?),
                    children: vec![expr],
                })
            }
            other => Err(self.error(format!("unknown dispatch token {other:?}"))),
        }
    }

    fn read_construct(&mut self, start: usize, depth: usize) -> Result<LocatedExprTree> {
        self.cx.read_policy.require(&read_construct_capability())?;

        let mut items: Vec<LocatedExprTree> = Vec::new();
        loop {
            let Some(token) = self.peek().cloned() else {
                return Err(self.error("unexpected end of read constructor"));
            };
            if token.kind == LispTokenKind::CloseParen {
                let close = self.next()?;
                let Some((head, tail)) = items.split_first() else {
                    return Err(self.error("empty read constructor"));
                };
                let class_symbol: &Symbol = match &head.expr {
                    Expr::Symbol(symbol) => symbol,
                    _ => return Err(self.error("read constructor head must be a class symbol")),
                };
                let args = tail
                    .iter()
                    .map(|expr| self.decode_read_construct_arg(expr.expr.clone()))
                    .collect::<Result<Vec<_>>>()?;
                let expr = self
                    .cx
                    .cx
                    .read_construct(class_symbol, args)?
                    .object()
                    .as_expr(self.cx.cx)?;
                return Ok(LocatedExprTree {
                    expr,
                    origin: Some(self.origin(start, close.end)?),
                    children: items,
                });
            }
            self.budget
                .check_collection_len(self.cx.codec, items.len() + 1)?;
            items.push(self.read_one(depth)?);
        }
    }

    fn read_eval(&mut self, start: usize, depth: usize) -> Result<LocatedExprTree> {
        self.cx.read_policy.require(&read_eval_capability())?;
        let mut items: Vec<LocatedExprTree> = Vec::new();
        loop {
            let Some(token) = self.peek().cloned() else {
                return Err(self.error("unexpected end of #eval group"));
            };
            if token.kind == LispTokenKind::CloseParen {
                let close = self.next()?;
                let expr = match items.as_slice() {
                    [] => return Err(self.error("empty #eval group")),
                    [one] => one.expr.clone(),
                    _ => lower_eval_surface(Expr::List(
                        items.iter().map(|item| item.expr.clone()).collect(),
                    )),
                };
                let value = self.eval_read_expr(expr)?;
                return Ok(LocatedExprTree {
                    expr: value,
                    origin: Some(self.origin(start, close.end)?),
                    children: items,
                });
            }
            self.budget
                .check_collection_len(self.cx.codec, items.len() + 1)?;
            items.push(self.read_one(depth)?);
        }
    }

    fn eval_read_expr(&mut self, expr: Expr) -> Result<Expr> {
        let value = self.cx.cx.eval_expr(lower_eval_surface(expr))?;
        value.object().as_expr(self.cx.cx)
    }

    fn decode_read_construct_arg(&mut self, expr: Expr) -> Result<Value> {
        decode_data_expr(self.cx, expr)
    }

    fn origin(&mut self, start: usize, end: usize) -> Result<Origin> {
        self.origin_with_trivia(start, end, Vec::new())
    }

    fn origin_with_trivia(
        &mut self,
        start: usize,
        end: usize,
        trivia: Vec<Trivia>,
    ) -> Result<Origin> {
        for _ in &trivia {
            self.budget.add_trivia(self.cx.codec)?;
        }
        Ok(Origin {
            codec: self.cx.codec,
            source: self.source_id.clone(),
            span: Span { start, end },
            trivia,
        })
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error::CodecError {
            codec: self.cx.codec,
            message: message.into(),
        }
    }
}
