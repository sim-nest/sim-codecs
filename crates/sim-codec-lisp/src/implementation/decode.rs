//! Decoding entry points for the Lisp codec: lexes and reads s-expression
//! source (or a `proc-macro2` token stream) into checked `Expr` forms and
//! located expression trees, lowering the eval surface as it goes.

use proc_macro2::{Delimiter, Group, Literal, TokenStream, TokenTree};
use sim_codec::{DecodeBudget, Decoder, Input, LocatedDecoder, ReadCx, TreeDecoder};
use sim_kernel::{
    Error, Expr, LocatedExpr, LocatedExprTree, QuoteMode, Result, SourceId, Symbol, Value,
    read_construct_capability, read_eval_capability,
};

use super::forms::{
    decode_data_expr, lower_eval_surface, may_be_number_literal, parse_byte_string_literal,
    parse_logic_var, parse_string_literal, parse_symbol, read_escape_form, read_explicit_quote,
};
use super::lex::{
    lex_lisp_tokens, lex_lisp_tokens_without_trivia, origin_from_lisp_source,
    strip_lisp_line_comments_preserve_layout,
};
use super::tree::LispTreeReader;

/// Returns the type name of the `proc-macro2` token stream this codec accepts as
/// pre-lexed input, used by the runtime to recognize token-stream decode inputs.
pub fn token_stream_type_name() -> &'static str {
    core::any::type_name::<proc_macro2::TokenStream>()
}

/// Lisp decoder built on `proc-macro2` tokenization.
///
/// Implements the codec's [`Decoder`], [`LocatedDecoder`], and [`TreeDecoder`]
/// roles: it lexes s-expression source and reads it into checked [`Expr`] forms,
/// located expressions, or located expression trees, lowering the eval surface
/// as it goes.
pub struct LispProcMacroDecoder;

impl Decoder for LispProcMacroDecoder {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input.into_string()?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let tokens = lex_lisp_tokens_without_trivia(cx.codec, &source, &mut budget)?;
        budget.check_tokens(cx.codec, tokens.len())?;
        let mut reader = LispTreeReader::new(
            cx,
            SourceId("<lisp-memory>".to_owned()),
            &source,
            tokens,
            &mut budget,
        );
        let expr = reader.read_one(0)?.expr;
        if !reader.is_empty() {
            return Err(Error::CodecError {
                codec: cx.codec,
                message: "expected exactly one top-level expression".to_owned(),
            });
        }
        Ok(expr)
    }
}

impl LocatedDecoder for LispProcMacroDecoder {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr> {
        decode_lisp_located(cx, source_id, input)
    }
}

impl TreeDecoder for LispProcMacroDecoder {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        decode_lisp_tree(cx, source_id, input)
    }
}

/// Decodes Lisp source into a [`LocatedExpr`], interning the source text under
/// `source_id` and attaching span origins so diagnostics can point back at it.
pub fn decode_lisp_located(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExpr> {
    let source = input.into_string()?;
    let mut budget = DecodeBudget::new(cx.limits);
    budget.check_input_bytes(cx.codec, source.len())?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let normalized = strip_lisp_line_comments_preserve_layout(&source);
    let stream = normalized
        .parse::<TokenStream>()
        .map_err(|err| Error::CodecError {
            codec: cx.codec,
            message: err.to_string(),
        })?;
    let tokens = stream.into_iter().collect::<Vec<_>>();
    budget.check_tokens(cx.codec, tokens.len())?;
    let mut reader = LispReader::new(cx, tokens, &mut budget);
    let expr = reader.read_one(0)?;
    if !reader.is_empty() {
        return Err(Error::CodecError {
            codec: cx.codec,
            message: "expected exactly one top-level expression".to_owned(),
        });
    }

    Ok(LocatedExpr {
        expr,
        origin: Some(origin_from_lisp_source(cx.codec, source_id, &source)),
    })
}

/// Decodes Lisp source into a [`LocatedExprTree`], preserving trivia and span
/// information for every node so the tree can be re-encoded losslessly.
pub fn decode_lisp_tree(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExprTree> {
    let source = input.into_string()?;
    let mut budget = DecodeBudget::new(cx.limits);
    budget.check_input_bytes(cx.codec, source.len())?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let tokens = lex_lisp_tokens(cx.codec, &source, &mut budget)?;
    budget.check_tokens(cx.codec, tokens.len())?;
    let mut reader = LispTreeReader::new(cx, source_id.clone(), &source, tokens, &mut budget);
    let mut tree = reader.read_one(0)?;
    if !reader.is_empty() {
        return Err(Error::CodecError {
            codec: cx.codec,
            message: "expected exactly one top-level expression".to_owned(),
        });
    }
    tree.origin = Some(origin_from_lisp_source(cx.codec, source_id, &source));
    Ok(tree)
}

struct LispReader<'a, 'cx, 'b> {
    cx: &'a mut ReadCx<'cx>,
    budget: &'b mut sim_codec::DecodeBudget,
    tokens: Vec<TokenTree>,
    index: usize,
}

impl<'a, 'cx, 'b> LispReader<'a, 'cx, 'b> {
    fn new(
        cx: &'a mut ReadCx<'cx>,
        tokens: Vec<TokenTree>,
        budget: &'b mut sim_codec::DecodeBudget,
    ) -> Self {
        Self {
            cx,
            budget,
            tokens,
            index: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn peek(&self) -> Option<&TokenTree> {
        self.tokens.get(self.index)
    }

    fn next(&mut self) -> Result<TokenTree> {
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

    fn read_one(&mut self, depth: usize) -> Result<Expr> {
        let token = self.next()?;
        self.read_token(token, depth)
    }

    fn read_token(&mut self, token: TokenTree, depth: usize) -> Result<Expr> {
        match token {
            TokenTree::Group(group) => self.read_group(group, depth),
            TokenTree::Literal(literal) => self.read_literal(literal, depth),
            TokenTree::Punct(punct) if punct.as_char() == '\'' => {
                self.budget.enter_node(self.cx.codec, depth)?;
                let expr = self.read_one(depth + 1)?;
                Ok(Expr::Quote {
                    mode: QuoteMode::Quote,
                    expr: Box::new(expr),
                })
            }
            TokenTree::Punct(punct) if punct.as_char() == '#' => self.read_dispatch(depth),
            token => self.read_symbolish(token, depth),
        }
    }

    fn read_group(&mut self, group: Group, depth: usize) -> Result<Expr> {
        let inner = group.stream().into_iter().collect::<Vec<_>>();
        let mut nested = LispReader::new(self.cx, inner, self.budget);
        let mut items: Vec<Expr> = Vec::new();
        while !nested.is_empty() {
            nested
                .budget
                .check_collection_len(nested.cx.codec, items.len() + 1)?;
            items.push(nested.read_one(depth + 1)?);
        }
        self.budget.enter_node(self.cx.codec, depth)?;

        match group.delimiter() {
            Delimiter::Parenthesis => {
                if let Some(quoted) = read_explicit_quote(&items) {
                    Ok(quoted)
                } else if let Some(expr) = read_escape_form(&items)? {
                    Ok(expr)
                } else {
                    Ok(Expr::List(items))
                }
            }
            Delimiter::Bracket => Ok(Expr::Vector(items)),
            Delimiter::Brace | Delimiter::None => Ok(Expr::Block(items)),
        }
    }

    fn read_literal(&mut self, literal: Literal, depth: usize) -> Result<Expr> {
        self.budget.enter_node(self.cx.codec, depth)?;
        let raw = literal.to_string();
        if raw.starts_with('"') {
            let value = parse_string_literal(self.cx.codec, &raw)?;
            self.budget.check_string_bytes(self.cx.codec, value.len())?;
            return Ok(Expr::String(value));
        }
        if raw.starts_with("b\"") {
            let value = parse_byte_string_literal(&raw)?;
            self.budget.check_blob_bytes(self.cx.codec, value.len())?;
            return Ok(Expr::Bytes(value));
        }
        if raw == "true" {
            return Ok(Expr::Bool(true));
        }
        if raw == "false" {
            return Ok(Expr::Bool(false));
        }
        let mut candidate = raw.clone();
        if may_be_number_literal(&candidate) {
            while let Some(next) = self.peek() {
                if !continues_number_literal(&candidate, next) {
                    break;
                }
                let fragment = token_to_symbol_fragment(next);
                let joined = format!("{candidate}{fragment}");
                self.next()?;
                candidate = joined;
            }
        }
        if may_be_number_literal(&candidate)
            && let Some(number) = self.cx.cx.parse_number_literal(&candidate)?
        {
            return Ok(Expr::Number(number));
        }
        Ok(Expr::Symbol(Symbol::new(candidate)))
    }

    fn read_symbolish(&mut self, first: TokenTree, depth: usize) -> Result<Expr> {
        self.budget.enter_node(self.cx.codec, depth)?;
        let mut text = token_to_symbol_fragment(&first);
        while let Some(next) = self.peek() {
            if !continues_symbol(text.as_str(), next) {
                break;
            }
            text.push_str(&token_to_symbol_fragment(&self.next()?));
        }

        match text.as_str() {
            "nil" => Ok(Expr::Nil),
            "true" => Ok(Expr::Bool(true)),
            "false" => Ok(Expr::Bool(false)),
            _ if parse_logic_var(&text).is_some() => Ok(parse_logic_var(&text).unwrap()),
            _ => Ok(Expr::Symbol(parse_symbol(&text))),
        }
    }

    fn read_dispatch(&mut self, depth: usize) -> Result<Expr> {
        self.budget.enter_node(self.cx.codec, depth)?;
        let token = self.next()?;
        match token {
            TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis => {
                self.read_construct(group, depth + 1)
            }
            TokenTree::Ident(ident) if ident == "eval" => {
                let token = self.next()?;
                let TokenTree::Group(group) = token else {
                    return Err(self.error("expected #eval(...)"));
                };
                self.read_eval(group, depth + 1)
            }
            TokenTree::Punct(punct) if punct.as_char() == '.' => {
                // `#.` is Common Lisp read-time eval; gate it on the same
                // capability as `#eval`/`#(...)`, which is otherwise theater
                // while `#.` evaluates untrusted input unguarded beside them.
                self.cx.read_policy.require(&read_eval_capability())?;
                let expr = self.read_one(depth + 1)?;
                self.eval_read_expr(expr)
            }
            other => Err(self.error(format!("unknown dispatch token {other}"))),
        }
    }

    fn read_construct(&mut self, group: Group, depth: usize) -> Result<Expr> {
        self.cx.read_policy.require(&read_construct_capability())?;

        let form = self.read_group(group, depth)?;
        let Expr::List(items) = form else {
            return Err(self.error("read constructor must be a list"));
        };
        let Some((head, tail)) = items.split_first() else {
            return Err(self.error("empty read constructor"));
        };
        let Expr::Symbol(class_symbol) = head else {
            return Err(self.error("read constructor head must be a class symbol"));
        };

        let args = tail
            .iter()
            .cloned()
            .map(|expr| self.decode_read_construct_arg(expr))
            .collect::<Result<Vec<_>>>()?;
        let value = self.cx.cx.read_construct(class_symbol, args)?;
        value.object().as_expr(self.cx.cx)
    }

    fn read_eval(&mut self, group: Group, depth: usize) -> Result<Expr> {
        self.cx.read_policy.require(&read_eval_capability())?;
        let inner = group.stream().into_iter().collect::<Vec<_>>();
        let mut nested = LispReader::new(self.cx, inner, self.budget);
        let mut items: Vec<Expr> = Vec::new();
        while !nested.is_empty() {
            nested
                .budget
                .check_collection_len(nested.cx.codec, items.len() + 1)?;
            items.push(nested.read_one(depth)?);
        }
        let expr = match items.as_slice() {
            [] => return Err(self.error("empty #eval group")),
            [one] => one.clone(),
            _ => lower_eval_surface(Expr::List(items)),
        };
        self.eval_read_expr(expr)
    }

    fn eval_read_expr(&mut self, expr: Expr) -> Result<Expr> {
        let value = self.cx.cx.eval_expr(lower_eval_surface(expr))?;
        value.object().as_expr(self.cx.cx)
    }

    fn decode_read_construct_arg(&mut self, expr: Expr) -> Result<Value> {
        decode_data_expr(self.cx, expr)
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error::CodecError {
            codec: self.cx.codec,
            message: message.into(),
        }
    }
}

fn continues_symbol(current: &str, next: &TokenTree) -> bool {
    match next {
        TokenTree::Ident(_) => current.ends_with(['/', ':', '?', '!', '-', '+', '.']),
        TokenTree::Punct(punct) => {
            matches!(punct.as_char(), '/' | ':' | '?' | '!' | '-' | '+' | '.')
        }
        _ => false,
    }
}

fn continues_number_literal(current: &str, next: &TokenTree) -> bool {
    match next {
        TokenTree::Punct(punct) => {
            let joined = format!("{}{}", current, punct.as_char());
            matches!(punct.as_char(), '+' | '-' | '/' | '.') && may_be_number_literal(&joined)
        }
        TokenTree::Ident(ident) => {
            let joined = format!("{current}{ident}");
            current.chars().any(|ch| ch.is_ascii_digit()) && may_be_number_literal(&joined)
        }
        TokenTree::Literal(literal) => {
            if !current.ends_with(['+', '-', '/', '.']) {
                return false;
            }
            let joined = format!("{current}{literal}");
            may_be_number_literal(&joined)
        }
        TokenTree::Group(_) => false,
    }
}

fn token_to_symbol_fragment(token: &TokenTree) -> String {
    match token {
        TokenTree::Group(group) => group.to_string(),
        TokenTree::Ident(ident) => ident.to_string(),
        TokenTree::Literal(literal) => literal.to_string(),
        TokenTree::Punct(punct) => punct.as_char().to_string(),
    }
}
