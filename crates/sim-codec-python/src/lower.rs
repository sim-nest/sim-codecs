//! Stable `python/*` forms and the three codec lanes.

use sim_codec::{DecodeBudget, Input, Output, ReadCx};
use sim_kernel::{
    CodecId, Error, Expr, LocatedExpr, LocatedExprTree, Origin, Result, SourceId,
    Span as KernelSpan, Symbol,
};

use crate::{Limits, Node, NodeKind, SyntaxTree, Token, TokenKind, parse_module_with_limits};

const FALLBACK_HEAD: &str = "__sim_expr__(";

/// Lower a complete concrete tree to the stable `python/module`,
/// `python/statement`, and `python/token` vocabulary. Parser coverage and
/// executable support are deliberately distinct: each token carries a final
/// boolean saying whether the future Python runtime profile may execute it.
pub fn lower_python(tree: &SyntaxTree) -> Expr {
    call(
        "module",
        tree.root
            .children
            .iter()
            .map(|node| lower_node(tree, node))
            .collect(),
    )
}

fn lower_node(tree: &SyntaxTree, node: &Node) -> Expr {
    let head = match node.kind {
        NodeKind::Module => "module",
        NodeKind::Statement => "statement",
        NodeKind::Suite => "suite",
        NodeKind::Group => "group",
        NodeKind::Expression => "expression",
    };
    let tokens = node
        .tokens
        .clone()
        .map(|index| lower_token(tree, &tree.tokens[index]));
    call(
        head,
        tokens
            .chain(node.children.iter().map(|child| {
                let child_head = match child.kind {
                    NodeKind::Module => "module",
                    NodeKind::Statement => "statement",
                    NodeKind::Suite => "suite",
                    NodeKind::Group => "group",
                    NodeKind::Expression => "expression",
                };
                call(child_head, Vec::new())
            }))
            .collect(),
    )
}

fn lower_token(tree: &SyntaxTree, token: &Token) -> Expr {
    let text = tree.source()[token.span.start..token.span.end].to_owned();
    call(
        "token",
        vec![
            Expr::Symbol(Symbol::new(token_kind_name(&token.kind))),
            Expr::String(text),
            Expr::Bool(executable_token(&token.kind)),
        ],
    )
}

fn token_kind_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::Name => "name",
        TokenKind::Keyword => "keyword",
        TokenKind::Number => "number",
        TokenKind::String => "string",
        TokenKind::FString => "f-string",
        TokenKind::TemplateString => "template-string",
        TokenKind::Operator => "operator",
        TokenKind::Newline => "newline",
        TokenKind::Indent => "indent",
        TokenKind::Dedent => "dedent",
        TokenKind::Trivia => "trivia",
        TokenKind::End => "end",
    }
}

fn executable_token(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Name | TokenKind::Number | TokenKind::String | TokenKind::Operator
    )
}

/// Decode Python source with the shared codec budget.
pub fn decode_python(cx: &mut ReadCx<'_>, source: &str, budget: &mut DecodeBudget) -> Result<Expr> {
    budget.check_input_bytes(cx.codec, source.len())?;
    if source.starts_with(FALLBACK_HEAD) {
        return decode_fallback(cx.codec, source, budget);
    }
    let tree = parse_module_with_limits(source, parser_limits(budget))
        .map_err(|error| codec_error(cx.codec, error.to_string()))?;
    budget.check_tokens(cx.codec, tree.tokens.len())?;
    Ok(lower_python(&tree))
}

/// Decode into a root-located expression.
pub fn decode_python_located(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExpr> {
    let source = input_text(cx.codec, input)?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let mut budget = DecodeBudget::new(cx.limits);
    let expr = decode_python(cx, &source, &mut budget)?;
    Ok(LocatedExpr {
        expr,
        origin: Some(origin(cx.codec, source_id, 0, source.len())),
    })
}

/// Decode into a source-origin tree. The root owns the file, statements own
/// their exact ranges, and token children retain the leaf spans.
pub fn decode_python_tree(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExprTree> {
    let source = input_text(cx.codec, input)?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let mut budget = DecodeBudget::new(cx.limits);
    if source.starts_with(FALLBACK_HEAD) {
        let expr = decode_python(cx, &source, &mut budget)?;
        let mut tree = LocatedExprTree::from_expr_recursive(expr);
        tree.origin = Some(origin(cx.codec, source_id, 0, source.len()));
        return Ok(tree);
    }
    let parsed = parse_module_with_limits(&source, parser_limits(&budget))
        .map_err(|error| codec_error(cx.codec, error.to_string()))?;
    budget.check_tokens(cx.codec, parsed.tokens.len())?;
    let expr = lower_python(&parsed);
    let mut root = LocatedExprTree::from_expr_recursive(expr);
    root.origin = Some(origin(cx.codec, source_id.clone(), 0, source.len()));
    for (child, statement) in root.children.iter_mut().skip(1).zip(&parsed.root.children) {
        if let Some((start, end)) = node_bytes(&parsed, statement) {
            child.origin = Some(origin(cx.codec, source_id.clone(), start, end));
            for (leaf, token_index) in child
                .children
                .iter_mut()
                .skip(1)
                .zip(statement.tokens.clone())
            {
                let span = parsed.tokens[token_index].span;
                leaf.origin = Some(origin(cx.codec, source_id.clone(), span.start, span.end));
            }
        }
    }
    Ok(root)
}

/// Canonically encode lowered Python forms. Expressions outside the stable
/// vocabulary use the established canonical tagged JSON projection inside a
/// reserved Python call, keeping `codec/python` general-purpose.
pub fn encode_python(expr: &Expr) -> Result<Output> {
    if python_call(expr).is_some() {
        let mut out = String::new();
        encode_form(expr, &mut out)?;
        let reparsed = parse_module_with_limits(&out, Limits::default())
            .map_err(|error| codec_error(crate::PYTHON_CODEC_ID, error.to_string()))?;
        if lower_python(&reparsed) != *expr {
            return Err(codec_error(
                crate::PYTHON_CODEC_ID,
                "python form is not the canonical lowering of its source",
            ));
        }
        return Ok(Output::Text(out));
    }
    let json = sim_codec_json::expr_to_json(expr);
    Ok(Output::Text(format!("{FALLBACK_HEAD}{json})")))
}

fn encode_form(expr: &Expr, out: &mut String) -> Result<()> {
    let Some((head, args)) = python_call(expr) else {
        return Err(codec_error(crate::PYTHON_CODEC_ID, "malformed python form"));
    };
    match head {
        "module" | "statement" | "suite" | "group" | "expression" => {
            for arg in args {
                encode_form(arg, out)?;
            }
            Ok(())
        }
        "token" if args.len() == 3 => match (&args[0], &args[1], &args[2]) {
            (Expr::Symbol(_), Expr::String(text), Expr::Bool(_)) => {
                out.push_str(text);
                Ok(())
            }
            _ => Err(codec_error(
                crate::PYTHON_CODEC_ID,
                "python/token expects kind, text, executable",
            )),
        },
        _ => Err(codec_error(
            crate::PYTHON_CODEC_ID,
            format!("unknown python form python/{head}"),
        )),
    }
}

fn decode_fallback(codec: CodecId, source: &str, budget: &mut DecodeBudget) -> Result<Expr> {
    let Some(json) = source
        .strip_prefix(FALLBACK_HEAD)
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Err(codec_error(codec, "malformed __sim_expr__ fallback"));
    };
    let value = serde_json::from_str(json)
        .map_err(|error| codec_error(codec, format!("malformed tagged fallback: {error}")))?;
    sim_codec_json::json_to_expr(codec, &value, budget, 0)
}

fn parser_limits(budget: &DecodeBudget) -> Limits {
    let limits = budget.limits();
    Limits {
        max_bytes: limits.max_input_bytes,
        max_tokens: limits.max_tokens,
        max_nesting: limits.max_depth,
        max_lines: limits.max_tokens,
    }
}

fn node_bytes(tree: &SyntaxTree, node: &Node) -> Option<(usize, usize)> {
    let tokens = &tree.tokens[node.tokens.clone()];
    Some((tokens.first()?.span.start, tokens.last()?.span.end))
}

fn python_call(expr: &Expr) -> Option<(&str, &[Expr])> {
    let Expr::Call { operator, args } = expr else {
        return None;
    };
    let Expr::Symbol(symbol) = operator.as_ref() else {
        return None;
    };
    (symbol.namespace.as_deref().map(AsRef::as_ref) == Some("python"))
        .then_some((symbol.name.as_ref(), args))
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("python", name))),
        args,
    }
}

fn input_text(codec: CodecId, input: Input) -> Result<String> {
    match input {
        Input::Text(text) => Ok(text),
        Input::Bytes(bytes) => String::from_utf8(bytes).map_err(|error| {
            codec_error(codec, format!("codec input is not valid UTF-8: {error}"))
        }),
    }
}

fn origin(codec: CodecId, source: SourceId, start: usize, end: usize) -> Origin {
    Origin {
        codec,
        source,
        span: KernelSpan { start, end },
        trivia: Vec::new(),
    }
}

fn codec_error(codec: CodecId, message: impl Into<String>) -> Error {
    Error::CodecError {
        codec,
        message: message.into(),
    }
}
