//! TypeScript overlay scanner over the public JavaScript seam.

use sim_codec_javascript::{Node, NodeKind, Span, TokenKind, tokenize_with_limits};

use crate::{Diagnostic, DiagnosticCode, Language, Limits, SyntaxKind, SyntaxNode, SyntaxTree};

/// Parse a TypeScript module.
pub fn parse_module(source: &str) -> Result<SyntaxTree, Diagnostic> {
    parse_module_with_limits(source, Limits::default())
}
/// Parse a TypeScript module with explicit bounds.
pub fn parse_module_with_limits(source: &str, limits: Limits) -> Result<SyntaxTree, Diagnostic> {
    parse(source, Language::TypeScript, limits)
}
/// Parse a TSX module.
pub fn parse_tsx(source: &str) -> Result<SyntaxTree, Diagnostic> {
    parse_tsx_with_limits(source, Limits::default())
}
/// Parse a TSX module with explicit bounds.
pub fn parse_tsx_with_limits(source: &str, limits: Limits) -> Result<SyntaxTree, Diagnostic> {
    parse(source, Language::Tsx, limits)
}

fn parse(source: &str, language: Language, limits: Limits) -> Result<SyntaxTree, Diagnostic> {
    if source.len() > limits.max_bytes {
        return Err(error(
            source,
            DiagnosticCode::ResourceLimit,
            0,
            "source byte limit exceeded",
        ));
    }
    let js_limits = sim_codec_javascript::Limits {
        max_bytes: limits.max_bytes,
        max_tokens: limits.max_nodes,
        max_nesting: limits.max_nesting,
        ..sim_codec_javascript::Limits::default()
    };
    let tokens = tokenize_with_limits(source, js_limits).map_err(|diagnostic| Diagnostic {
        code: match diagnostic.code {
            sim_codec_javascript::DiagnosticCode::ResourceLimit => DiagnosticCode::ResourceLimit,
            _ => DiagnosticCode::UnclosedSyntax,
        },
        span: diagnostic.span,
        line: diagnostic.line,
        column: diagnostic.column,
        message: diagnostic.message,
    })?;
    let mut nodes = vec![SyntaxNode::JavaScript(Node {
        kind: NodeKind::Module,
        tokens: 0..tokens.len(),
        children: Vec::new(),
        asi: None,
    })];
    let mut context = Vec::<String>::new();
    for (index, token) in tokens.iter().enumerate() {
        let word = &source[token.span.start..token.span.end];
        if matches!(
            word,
            "class" | "interface" | "type" | "enum" | "namespace" | "module" | "function"
        ) {
            context.clear();
            context.push(word.to_owned());
        }
        let kind = if matches!(
            word,
            "interface" | "type" | "enum" | "namespace" | "declare"
        ) {
            Some(SyntaxKind::Declaration)
        } else if matches!(
            word,
            "public"
                | "private"
                | "protected"
                | "readonly"
                | "abstract"
                | "override"
                | "accessor"
                | "declare"
        ) {
            Some(SyntaxKind::Modifier)
        } else if matches!(
            word,
            "keyof"
                | "infer"
                | "is"
                | "asserts"
                | "satisfies"
                | "typeof"
                | "unique"
                | "unknown"
                | "never"
                | "any"
        ) {
            Some(SyntaxKind::TypeNode)
        } else if word == ":" {
            Some(SyntaxKind::Annotation)
        } else if word == "<" && looks_like_jsx(source, token.span.start) {
            if language == Language::TypeScript {
                return Err(error(
                    source,
                    DiagnosticCode::JsxInTypeScript,
                    token.span.start,
                    "JSX requires TSX mode",
                ));
            }
            Some(SyntaxKind::Jsx)
        } else if word == "<" && looks_like_type_arguments(&tokens, index, source) {
            Some(SyntaxKind::TypeArguments)
        } else {
            None
        };
        if let Some(kind) = kind {
            nodes.push(SyntaxNode::TypeScript {
                kind,
                span: token.span,
                context: context.clone(),
            });
            if nodes.len() > limits.max_nodes {
                return Err(error(
                    source,
                    DiagnosticCode::ResourceLimit,
                    token.span.start,
                    "node limit exceeded",
                ));
            }
        }
    }
    Ok(SyntaxTree {
        source: source.to_owned(),
        language,
        tokens,
        nodes,
    })
}

fn looks_like_jsx(source: &str, at: usize) -> bool {
    let tail = &source[at + 1..];
    tail.starts_with('>')
        || tail.starts_with('/')
        || (tail
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
            && (tail.contains("</") || tail.contains("/>")))
}
fn looks_like_type_arguments(
    tokens: &[sim_codec_javascript::Token],
    index: usize,
    source: &str,
) -> bool {
    tokens.get(index + 1).is_some_and(|next| {
        let text = &source[next.span.start..next.span.end];
        next.kind == TokenKind::Identifier
            && !looks_like_jsx(source, tokens[index].span.start)
            && !text.is_empty()
    })
}
fn error(source: &str, code: DiagnosticCode, at: usize, message: &str) -> Diagnostic {
    let prefix = &source[..at.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count();
    Diagnostic {
        code,
        span: Span { start: at, end: at },
        line,
        column,
        message: message.to_owned(),
    }
}
