//! Stable `javascript/*` forms and public lowering builders.

use sim_codec::{DecodeBudget, Input, Output, ReadCx};
use sim_kernel::{
    CodecId, Error, Expr, LocatedExpr, LocatedExprTree, Origin as KernelOrigin, Result, SourceId,
    Span as KernelSpan, Symbol,
};

use crate::{
    Goal, Limits, Node, NodeKind, Origin, SyntaxTree, Token, TokenKind, parse_module_with_limits,
    parse_script_with_limits,
};

const FALLBACK_HEAD: &str = "__sim_expr__(";

/// Public construction seam for JavaScript and syntax extensions.
#[derive(Clone, Debug, Default)]
pub struct JavascriptBuilder;

impl JavascriptBuilder {
    /// Construct a stable namespaced JavaScript form.
    #[must_use]
    pub fn form(&self, name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("javascript", name))),
            args,
        }
    }

    /// Construct a token form shared by the built-in lowering and extensions.
    #[must_use]
    pub fn token(&self, kind: &str, text: impl Into<String>, executable: bool) -> Expr {
        self.form(
            "token",
            vec![
                Expr::Symbol(Symbol::new(kind)),
                Expr::String(text.into()),
                Expr::Bool(executable),
            ],
        )
    }

    /// Attach a caller-owned origin whose parent retains the earlier transform.
    #[must_use]
    pub fn derived_origin(
        &self,
        source: impl Into<String>,
        span: crate::Span,
        parent: Option<Origin>,
    ) -> Origin {
        Origin {
            source: source.into(),
            span,
            parent: parent.map(Box::new),
        }
    }

    /// Lower one public node, enabling downstream node wrappers without parser copying.
    #[must_use]
    pub fn node(&self, node: &Node) -> Expr {
        lower_node(self, node)
    }
}

/// Lower a complete tree to stable `javascript/*` forms.
#[must_use]
pub fn lower_javascript(tree: &SyntaxTree) -> Expr {
    let builder = JavascriptBuilder;
    let tokens = tree
        .tokens
        .iter()
        .map(|token| lower_token(&builder, tree, token));
    builder.form(
        goal_name(tree.goal),
        tokens
            .chain(tree.root.children.iter().map(|node| builder.node(node)))
            .collect(),
    )
}

fn lower_node(builder: &JavascriptBuilder, node: &Node) -> Expr {
    builder.form(
        node_name(&node.kind),
        node.children
            .iter()
            .map(|child| lower_node(builder, child))
            .collect(),
    )
}

fn lower_token(builder: &JavascriptBuilder, tree: &SyntaxTree, token: &Token) -> Expr {
    builder.token(
        token_name(&token.kind),
        &tree.source()[token.span.start..token.span.end],
        executable_token(&token.kind),
    )
}

fn goal_name(goal: Goal) -> &'static str {
    match goal {
        Goal::Script => "script",
        Goal::Module => "module",
    }
}
fn node_name(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Script => "script",
        NodeKind::Module => "module",
        NodeKind::StatementList => "statement-list",
        NodeKind::Declaration => "declaration",
        NodeKind::Statement => "statement",
        NodeKind::Function => "function",
        NodeKind::Class => "class",
        NodeKind::Import => "import",
        NodeKind::Export => "export",
        NodeKind::Expression => "expression",
        NodeKind::Group => "group",
    }
}
fn token_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::Identifier => "identifier",
        TokenKind::Keyword => "keyword",
        TokenKind::Number => "number",
        TokenKind::String => "string",
        TokenKind::RegExp => "regexp",
        TokenKind::Template => "template",
        TokenKind::Punctuator => "punctuator",
        TokenKind::Trivia => "trivia",
        TokenKind::End => "end",
    }
}
fn executable_token(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::Number
            | TokenKind::String
            | TokenKind::RegExp
            | TokenKind::Template
            | TokenKind::Punctuator
    )
}

/// Decode Script source using the shared codec budget.
pub fn decode_javascript(
    cx: &mut ReadCx<'_>,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    budget.check_input_bytes(cx.codec, source.len())?;
    if source.starts_with(FALLBACK_HEAD) {
        return decode_fallback(cx.codec, source, budget);
    }
    let tree = parse_script_with_limits(source, parser_limits(budget))
        .map_err(|e| codec_error(cx.codec, e.to_string()))?;
    budget.check_tokens(cx.codec, tree.tokens.len())?;
    Ok(lower_javascript(&tree))
}

/// Decode into a root-located expression.
pub fn decode_javascript_located(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExpr> {
    let source = input_text(cx.codec, input)?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let mut budget = DecodeBudget::new(cx.limits);
    let expr = decode_javascript(cx, &source, &mut budget)?;
    Ok(LocatedExpr {
        expr,
        origin: Some(origin(cx.codec, source_id, 0, source.len())),
    })
}

/// Decode into a recursively located expression tree.
pub fn decode_javascript_tree(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExprTree> {
    let source = input_text(cx.codec, input)?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let mut budget = DecodeBudget::new(cx.limits);
    if source.starts_with(FALLBACK_HEAD) {
        let mut out =
            LocatedExprTree::from_expr_recursive(decode_javascript(cx, &source, &mut budget)?);
        out.origin = Some(origin(cx.codec, source_id, 0, source.len()));
        return Ok(out);
    }
    let parsed = parse_script_with_limits(&source, parser_limits(&budget))
        .map_err(|e| codec_error(cx.codec, e.to_string()))?;
    budget.check_tokens(cx.codec, parsed.tokens.len())?;
    let mut out = LocatedExprTree::from_expr_recursive(lower_javascript(&parsed));
    out.origin = Some(origin(cx.codec, source_id.clone(), 0, source.len()));
    for (leaf, token) in out.children.iter_mut().skip(1).zip(&parsed.tokens) {
        leaf.origin = Some(origin(
            cx.codec,
            source_id.clone(),
            token.span.start,
            token.span.end,
        ));
    }
    for (child, node) in out
        .children
        .iter_mut()
        .skip(1 + parsed.tokens.len())
        .zip(&parsed.root.children)
    {
        locate_node(child, &parsed, node, cx.codec, &source_id);
    }
    Ok(out)
}

fn locate_node(
    tree: &mut LocatedExprTree,
    syntax: &SyntaxTree,
    node: &Node,
    codec: CodecId,
    source: &SourceId,
) {
    if let Some((start, end)) = node_bytes(syntax, node) {
        tree.origin = Some(origin(codec, source.clone(), start, end));
    }
    for (child, child_node) in tree.children.iter_mut().skip(1).zip(&node.children) {
        locate_node(child, syntax, child_node, codec, source);
    }
}

fn node_bytes(tree: &SyntaxTree, node: &Node) -> Option<(usize, usize)> {
    let tokens = &tree.tokens[node.tokens.clone()];
    Some((tokens.first()?.span.start, tokens.last()?.span.end))
}

/// Canonically encode lowered forms or the established tagged fallback.
pub fn encode_javascript(expr: &Expr) -> Result<Output> {
    if javascript_call(expr).is_some() {
        let mut out = String::new();
        encode_form(expr, &mut out)?;
        let goal = javascript_call(expr).map(|x| x.0).unwrap_or_default();
        let parsed = if goal == "module" {
            parse_module_with_limits(&out, Limits::default())
        } else {
            parse_script_with_limits(&out, Limits::default())
        }
        .map_err(|e| codec_error(crate::JAVASCRIPT_CODEC_ID, e.to_string()))?;
        if lower_javascript(&parsed) != *expr {
            return Err(codec_error(
                crate::JAVASCRIPT_CODEC_ID,
                "javascript form is not the canonical lowering of its source",
            ));
        }
        return Ok(Output::Text(out));
    }
    Ok(Output::Text(format!(
        "{FALLBACK_HEAD}{})",
        sim_codec_json::expr_to_json(expr)
    )))
}

fn encode_form(expr: &Expr, out: &mut String) -> Result<()> {
    let Some((head, args)) = javascript_call(expr) else {
        return Err(codec_error(
            crate::JAVASCRIPT_CODEC_ID,
            "malformed javascript form",
        ));
    };
    if head == "token" && args.len() == 3 {
        if let (Expr::Symbol(_), Expr::String(text), Expr::Bool(_)) = (&args[0], &args[1], &args[2])
        {
            out.push_str(text);
            return Ok(());
        }
        return Err(codec_error(
            crate::JAVASCRIPT_CODEC_ID,
            "javascript/token expects kind, text, executable",
        ));
    }
    if matches!(
        head,
        "script"
            | "module"
            | "statement-list"
            | "declaration"
            | "statement"
            | "function"
            | "class"
            | "import"
            | "export"
            | "expression"
            | "group"
    ) {
        for arg in args {
            encode_form(arg, out)?;
        }
        return Ok(());
    }
    Err(codec_error(
        crate::JAVASCRIPT_CODEC_ID,
        format!("unknown javascript form javascript/{head}"),
    ))
}

fn decode_fallback(codec: CodecId, source: &str, budget: &mut DecodeBudget) -> Result<Expr> {
    let json = source
        .strip_prefix(FALLBACK_HEAD)
        .and_then(|x| x.strip_suffix(')'))
        .ok_or_else(|| codec_error(codec, "malformed __sim_expr__ fallback"))?;
    let value = serde_json::from_str(json)
        .map_err(|e| codec_error(codec, format!("malformed tagged fallback: {e}")))?;
    sim_codec_json::json_to_expr(codec, &value, budget, 0)
}
fn parser_limits(b: &DecodeBudget) -> Limits {
    let l = b.limits();
    Limits {
        max_bytes: l.max_input_bytes,
        max_tokens: l.max_tokens,
        max_nesting: l.max_depth,
        max_lines: l.max_tokens,
        max_nodes: l.max_tokens,
    }
}
fn javascript_call(expr: &Expr) -> Option<(&str, &[Expr])> {
    let Expr::Call { operator, args } = expr else {
        return None;
    };
    let Expr::Symbol(s) = operator.as_ref() else {
        return None;
    };
    (s.namespace.as_deref().map(AsRef::as_ref) == Some("javascript"))
        .then_some((s.name.as_ref(), args))
}
fn input_text(codec: CodecId, input: Input) -> Result<String> {
    match input {
        Input::Text(x) => Ok(x),
        Input::Bytes(x) => String::from_utf8(x)
            .map_err(|e| codec_error(codec, format!("codec input is not valid UTF-8: {e}"))),
    }
}
fn origin(codec: CodecId, source: SourceId, start: usize, end: usize) -> KernelOrigin {
    KernelOrigin {
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
