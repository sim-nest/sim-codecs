//! Direct, erasure-only lowering to the JavaScript expression graph.

use sim_codec::{DecodeBudget, Input, Output, ReadCx};
use sim_codec_javascript::{JavascriptBuilder, Origin, Span, Token, TokenKind};
use sim_kernel::{
    Error, Expr, LocatedExpr, LocatedExprTree, Origin as KernelOrigin, Result, SourceId,
    Span as KernelSpan,
};

use crate::{
    Language, Limits, SyntaxKind, SyntaxNode, SyntaxTree, TYPESCRIPT_CODEC_ID,
    parse_module_with_limits,
};

/// A retained reference to erased annotation syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationReference {
    /// Exact annotation location in the TypeScript source.
    pub span: Span,
    /// Parser context in which the annotation occurred.
    pub context: Vec<String>,
    /// Complete TypeScript-to-JavaScript derivation chain.
    pub origin: Origin,
}

/// A located reason why syntax cannot be erased without a compiler decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationGap {
    /// Exact unsupported syntax location.
    pub span: Span,
    /// Stable syntax category.
    pub construct: String,
    /// Literal admission-rule explanation.
    pub reason: String,
}

impl std::fmt::Display for EvaluationGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TypeScript evaluation gap at {}..{}: {} requires {}",
            self.span.start, self.span.end, self.construct, self.reason
        )
    }
}
impl std::error::Error for EvaluationGap {}

/// An admitted JavaScript graph plus lossless TypeScript provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweredTypeScript {
    /// The exact JavaScript expression graph built without emit or reparse.
    pub javascript: Expr,
    /// References to annotations erased from runtime syntax.
    pub annotations: Vec<AnnotationReference>,
    /// The original lossless TypeScript tree.
    pub source_tree: SyntaxTree,
}

/// Apply the literal "requires no compiler decision" predicate and lower directly.
pub fn lower_typescript(
    tree: &SyntaxTree,
) -> std::result::Result<LoweredTypeScript, EvaluationGap> {
    if let Some(gap) = first_gap(tree) {
        return Err(gap);
    }
    let erased = erased_tokens(tree);
    let builder = JavascriptBuilder;
    let javascript = builder.form(
        "module",
        erased
            .iter()
            .map(|token| {
                builder.token(
                    token_name(&token.kind),
                    &tree.source()[token.span.start..token.span.end],
                    executable(&token.kind),
                )
            })
            .collect(),
    );
    let annotations = tree
        .nodes
        .iter()
        .filter_map(|node| match node {
            SyntaxNode::TypeScript {
                kind: SyntaxKind::Annotation,
                span,
                context,
            } => Some(AnnotationReference {
                span: *span,
                context: context.clone(),
                origin: builder.derived_origin(
                    "typescript",
                    *span,
                    Some(builder.derived_origin("javascript", *span, None)),
                ),
            }),
            _ => None,
        })
        .collect();
    Ok(LoweredTypeScript {
        javascript,
        annotations,
        source_tree: tree.clone(),
    })
}

fn first_gap(tree: &SyntaxTree) -> Option<EvaluationGap> {
    let source = tree.source();
    for token in significant(&tree.tokens) {
        let text = token_text(source, token);
        let reason = match text {
            "enum" | "namespace" | "module" => {
                Some("an emitter transform and runtime code generation")
            }
            "satisfies" | "asserts" => Some("a type-checker acceptance decision"),
            "as" if !is_module_alias(source, token.span.start) => {
                Some("a type-checker acceptance decision")
            }
            "@" => Some("a decorator transform and target selection"),
            _ => None,
        };
        if let Some(reason) = reason {
            return Some(gap(token.span, text, reason));
        }
    }
    if tree.language == Language::Tsx
        && let Some(span) = tree.nodes.iter().find_map(|node| match node {
            SyntaxNode::TypeScript {
                kind: SyntaxKind::Jsx,
                span,
                ..
            } => Some(*span),
            _ => None,
        })
    {
        return Some(gap(
            span,
            "JSX/TSX",
            "a JSX emitter transform and target selection",
        ));
    }
    // Accessibility on constructor parameters creates fields and assignments.
    for window in significant(&tree.tokens).windows(2) {
        let first = token_text(source, window[0]);
        if matches!(first, "public" | "private" | "protected")
            && source[..window[0].span.start]
                .rsplit_once("constructor(")
                .is_some_and(|(_, tail)| !tail.contains(')'))
        {
            return Some(gap(
                window[0].span,
                "parameter property",
                "an emitter transform and runtime assignment",
            ));
        }
    }
    None
}

fn erased_tokens(tree: &SyntaxTree) -> Vec<&Token> {
    let tokens = significant(&tree.tokens);
    let source = tree.source();
    let mut erase = vec![false; tokens.len()];
    let mut index = 0;
    while index < tokens.len() {
        let text = token_text(source, tokens[index]);
        if matches!(text, "interface" | "type")
            || (text == "declare"
                && tokens.get(index + 1).is_some_and(|t| {
                    matches!(
                        token_text(source, t),
                        "interface" | "class" | "function" | "const" | "let" | "var"
                    )
                }))
        {
            let end = declaration_end(&tokens, source, index);
            erase[index..end].fill(true);
            index = end;
            continue;
        }
        if matches!(text, "readonly" | "abstract" | "override" | "declare") {
            erase[index] = true;
        }
        if text == ":" {
            let end = type_end(&tokens, source, index + 1);
            erase[index..end].fill(true);
            index = end;
            continue;
        }
        if text == "<" && looks_like_generic(&tokens, source, index) {
            let end = matching_angle(&tokens, source, index).map_or(index + 1, |x| x + 1);
            erase[index..end].fill(true);
            index = end;
            continue;
        }
        index += 1;
    }
    tree.tokens
        .iter()
        .filter(|token| {
            token.kind == TokenKind::Trivia
                || tokens
                    .iter()
                    .position(|candidate| std::ptr::eq(*candidate, *token))
                    .is_none_or(|i| !erase[i])
        })
        .collect()
}

fn is_module_alias(source: &str, at: usize) -> bool {
    let statement = source[..at]
        .rsplit_once([';', '\n'])
        .map_or(&source[..at], |(_, tail)| tail);
    statement
        .split_whitespace()
        .any(|word| matches!(word, "import" | "export"))
}

fn declaration_end(tokens: &[&Token], source: &str, start: usize) -> usize {
    let mut braces = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(start) {
        match token_text(source, token) {
            "{" => braces += 1,
            "}" if braces > 0 => {
                braces -= 1;
                if braces == 0 {
                    return i + 1;
                }
            }
            ";" if braces == 0 => return i + 1,
            _ => {}
        }
    }
    tokens.len()
}
fn type_end(tokens: &[&Token], source: &str, start: usize) -> usize {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(start) {
        match token_text(source, token) {
            "{" if depth == 0 && i > start => return i,
            "<" | "[" | "{" => depth += 1,
            ">" | "]" | "}" if depth > 0 => depth -= 1,
            "," | ")" | "=" | ";" if depth == 0 => return i,
            _ => {}
        }
    }
    tokens.len()
}
fn looks_like_generic(tokens: &[&Token], source: &str, at: usize) -> bool {
    at > 0
        && matches!(
            tokens[at - 1].kind,
            TokenKind::Identifier | TokenKind::Keyword
        )
        && matching_angle(tokens, source, at).is_some_and(|end| {
            tokens
                .get(end + 1)
                .is_some_and(|t| matches!(token_text(source, t), "(" | "."))
        })
}
fn matching_angle(tokens: &[&Token], source: &str, at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(at) {
        match token_text(source, token) {
            "<" => depth += 1,
            ">" => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}
fn significant(tokens: &[Token]) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Trivia | TokenKind::End))
        .collect()
}
fn token_text<'a>(source: &'a str, token: &Token) -> &'a str {
    &source[token.span.start..token.span.end]
}
fn gap(span: Span, construct: &str, reason: &str) -> EvaluationGap {
    EvaluationGap {
        span,
        construct: construct.into(),
        reason: reason.into(),
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
fn executable(kind: &TokenKind) -> bool {
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

/// Decode admitted TypeScript to its direct JavaScript graph.
pub fn decode_typescript(
    cx: &mut ReadCx<'_>,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    budget.check_input_bytes(cx.codec, source.len())?;
    if source.starts_with("__sim_expr__(") {
        return sim_codec_javascript::decode_javascript(cx, source, budget);
    }
    let tree = parse_module_with_limits(source, parser_limits(budget))
        .map_err(|e| codec_error(e.to_string()))?;
    budget.check_tokens(cx.codec, tree.tokens.len())?;
    lower_typescript(&tree)
        .map(|x| x.javascript)
        .map_err(|e| codec_error(e.to_string()))
}
/// Decode admitted TypeScript with a root source location.
pub fn decode_typescript_located(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExpr> {
    let source = input_text(input)?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let mut budget = DecodeBudget::new(cx.limits);
    let expr = decode_typescript(cx, &source, &mut budget)?;
    Ok(LocatedExpr {
        expr,
        origin: Some(origin(cx.codec, source_id, 0, source.len())),
    })
}
/// Decode admitted TypeScript into a recursively located JavaScript tree.
pub fn decode_typescript_tree(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExprTree> {
    let located = decode_typescript_located(cx, source_id, input)?;
    let mut tree = LocatedExprTree::from_expr_recursive(located.expr);
    tree.origin = located.origin;
    Ok(tree)
}
/// Canonically encode JavaScript forms as TypeScript-compatible source, with tagged fallback.
pub fn encode_typescript(expr: &Expr) -> Result<Output> {
    if is_javascript_form(expr) {
        let mut source = String::new();
        encode_direct(expr, &mut source)?;
        return Ok(Output::Text(source));
    }
    sim_codec_javascript::encode_javascript(expr)
}
fn is_javascript_form(expr: &Expr) -> bool {
    matches!(expr, Expr::Call { operator, .. } if matches!(operator.as_ref(), Expr::Symbol(symbol) if symbol.namespace.as_deref().map(AsRef::as_ref) == Some("javascript")))
}
fn encode_direct(expr: &Expr, source: &mut String) -> Result<()> {
    let Expr::Call { operator, args } = expr else {
        return Err(codec_error("malformed JavaScript form in TypeScript graph"));
    };
    let Expr::Symbol(symbol) = operator.as_ref() else {
        return Err(codec_error(
            "malformed JavaScript operator in TypeScript graph",
        ));
    };
    if symbol.namespace.as_deref().map(AsRef::as_ref) != Some("javascript") {
        return Err(codec_error("non-JavaScript child in TypeScript graph"));
    }
    if symbol.name.as_ref() == "token" {
        if let [Expr::Symbol(_), Expr::String(text), Expr::Bool(_)] = args.as_slice() {
            source.push_str(text);
            return Ok(());
        }
        return Err(codec_error(
            "javascript/token expects kind, text, executable",
        ));
    }
    for arg in args {
        encode_direct(arg, source)?;
    }
    Ok(())
}
fn parser_limits(b: &DecodeBudget) -> Limits {
    let l = b.limits();
    Limits {
        max_bytes: l.max_input_bytes,
        max_nodes: l.max_tokens,
        max_nesting: l.max_depth,
    }
}
fn input_text(input: Input) -> Result<String> {
    match input {
        Input::Text(x) => Ok(x),
        Input::Bytes(x) => String::from_utf8(x)
            .map_err(|e| codec_error(format!("codec input is not valid UTF-8: {e}"))),
    }
}
fn origin(codec: sim_kernel::CodecId, source: SourceId, start: usize, end: usize) -> KernelOrigin {
    KernelOrigin {
        codec,
        source,
        span: KernelSpan { start, end },
        trivia: Vec::new(),
    }
}
fn codec_error(message: impl Into<String>) -> Error {
    Error::CodecError {
        codec: TYPESCRIPT_CODEC_ID,
        message: message.into(),
    }
}
