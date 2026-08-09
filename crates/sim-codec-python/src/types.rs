//! Public, runtime-independent syntax data.

use std::fmt;

/// Half-open UTF-8 byte range in the original source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// Byte after the range.
    pub end: usize,
}

/// Kind of a lossless lexical token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    /// Identifier, including contextual soft-keyword spellings.
    Name,
    /// Reserved keyword.
    Keyword,
    /// Numeric literal with its original spelling retained.
    Number,
    /// Ordinary string or bytes literal.
    String,
    /// Formatted string literal.
    FString,
    /// Python 3.14 template string literal.
    TemplateString,
    /// Operator or delimiter.
    Operator,
    /// Physical line ending.
    Newline,
    /// Significant indentation increase.
    Indent,
    /// Significant indentation decrease (zero-width).
    Dedent,
    /// Spaces, tabs, form feeds, comments, or escaped newlines.
    Trivia,
    /// End marker (zero-width).
    End,
}

/// A token borrowing no source storage; text is recovered through [`Token::span`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    /// Token category.
    pub kind: TokenKind,
    /// Original byte range.
    pub span: Span,
    /// One-based line.
    pub line: usize,
    /// Zero-based Unicode-scalar column.
    pub column: usize,
}

/// Concrete source-tree node category.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeKind {
    /// Complete file input.
    Module,
    /// Logical statement.
    Statement,
    /// Indented suite.
    Suite,
    /// Delimited expression/grouping region.
    Group,
    /// Expression region whose precedence was admitted through the Pratt table.
    Expression,
}

/// A concrete node covering a contiguous token range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    /// Node category.
    pub kind: NodeKind,
    /// Half-open token-index range.
    pub tokens: std::ops::Range<usize>,
    /// Nested structural nodes.
    pub children: Vec<Node>,
}

/// Complete lossless Python source tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxTree {
    source: String,
    /// Lossless token stream, including trivia and layout markers.
    pub tokens: Vec<Token>,
    /// Structural root.
    pub root: Node,
}

impl SyntaxTree {
    /// Returns the exact input bytes as UTF-8 text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Re-emits source byte-for-byte. Zero-width layout markers contribute no bytes.
    #[must_use]
    pub fn preserve_source(&self) -> String {
        self.source.clone()
    }

    pub(crate) fn new(source: &str, tokens: Vec<Token>, root: Node) -> Self {
        Self {
            source: source.to_owned(),
            tokens,
            root,
        }
    }
}

/// Resource limits shared by lexing and parsing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum source bytes.
    pub max_bytes: usize,
    /// Maximum emitted tokens (including trivia/layout).
    pub max_tokens: usize,
    /// Maximum bracket, f-string, and indentation nesting.
    pub max_nesting: usize,
    /// Maximum physical lines.
    pub max_lines: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_tokens: 1_000_000,
            max_nesting: 256,
            max_lines: 250_000,
        }
    }
}

/// Stable diagnostic category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    /// A configured resource bound was crossed.
    ResourceLimit,
    /// Indentation did not match an earlier level.
    InvalidIndentation,
    /// A tab/space combination has an ambiguous visual column.
    AmbiguousIndentation,
    /// A literal was not terminated.
    UnterminatedLiteral,
    /// A character cannot begin a Python token.
    InvalidCharacter,
    /// Delimiters are unmatched or crossed.
    UnmatchedDelimiter,
    /// Statement structure is invalid.
    InvalidSyntax,
}

/// Deterministic, located syntax failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Stable category.
    pub code: DiagnosticCode,
    /// Offending source range.
    pub span: Span,
    /// One-based source line.
    pub line: usize,
    /// Zero-based Unicode-scalar column.
    pub column: usize,
    /// Stable human-readable detail.
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for Diagnostic {}
