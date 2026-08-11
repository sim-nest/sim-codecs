//! Public TypeScript extension data.

use sim_codec_javascript::{Node, Span, Token};

/// TypeScript or TSX input mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    /// TypeScript module notation.
    TypeScript,
    /// TypeScript with JSX notation.
    Tsx,
}

/// Resource limits for the extension scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum source bytes.
    pub max_bytes: usize,
    /// Maximum extension nodes.
    pub max_nodes: usize,
    /// Maximum delimiter nesting.
    pub max_nesting: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_nodes: 1_000_000,
            max_nesting: 256,
        }
    }
}

/// TypeScript-only concrete syntax category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxKind {
    /// `interface`, `type`, `enum`, `namespace`, or ambient declaration.
    Declaration,
    /// A `: Type` annotation.
    Annotation,
    /// A generic parameter or argument list.
    TypeArguments,
    /// A TypeScript type node or operator.
    TypeNode,
    /// A TypeScript declaration/member modifier.
    Modifier,
    /// A JSX element, fragment, attribute, expression, or text region.
    Jsx,
}

/// A syntax node in the composed tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxNode {
    /// An unchanged ECMAScript node produced by the JavaScript frontend.
    JavaScript(Node),
    /// TypeScript-only notation layered over JavaScript.
    TypeScript {
        /// Extension category.
        kind: SyntaxKind,
        /// Exact byte range in the source.
        span: Span,
        /// Parser context recorded at the node.
        context: Vec<String>,
    },
}

/// Complete lossless composed syntax tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxTree {
    pub(crate) source: String,
    /// Selected language mode.
    pub language: Language,
    /// Lossless JavaScript token seam, including trivia.
    pub tokens: Vec<Token>,
    /// JavaScript and TypeScript nodes.
    pub nodes: Vec<SyntaxNode>,
}
impl SyntaxTree {
    /// Returns the exact admitted input.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Re-emits the admitted input byte-for-byte.
    #[must_use]
    pub fn preserve_source(&self) -> String {
        self.source.clone()
    }
}

/// Stable diagnostic category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    /// A configured resource bound was crossed.
    ResourceLimit,
    /// Delimiters, strings, comments, templates, or JSX were not closed.
    UnclosedSyntax,
    /// JSX notation was used outside TSX mode.
    JsxInTypeScript,
}

/// Deterministic located frontend failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Stable category.
    pub code: DiagnosticCode,
    /// Offending source range.
    pub span: Span,
    /// One-based line.
    pub line: usize,
    /// Zero-based Unicode-scalar column.
    pub column: usize,
    /// Stable detail.
    pub message: String,
}
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}
impl std::error::Error for Diagnostic {}
