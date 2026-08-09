//! Runtime-independent public syntax and extension data.

use std::fmt;

/// Half-open UTF-8 byte range in the original source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// Byte after the range.
    pub end: usize,
}

/// Script or Module syntactic goal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Goal {
    /// Script goal.
    Script,
    /// Module goal (always strict).
    Module,
}

/// Parser-selected lexical goal governing slash and template recognition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexicalGoal {
    /// Division or division-assignment is admitted.
    Div,
    /// A regular-expression literal is admitted.
    RegExp,
    /// A template tail follows a substitution.
    TemplateTail,
}

/// Kind of lossless token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    /// IdentifierName, including escaped spelling.
    Identifier,
    /// Reserved or contextual keyword spelling.
    Keyword,
    /// Numeric literal.
    Number,
    /// String literal.
    String,
    /// Regular-expression literal.
    RegExp,
    /// No-substitution template or template segment.
    Template,
    /// Punctuator.
    Punctuator,
    /// Whitespace, line terminator, comment, or hashbang.
    Trivia,
    /// End marker.
    End,
}

/// A lossless lexical token. Text is recovered using [`Token::span`].
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
    /// Lexical goal used to recognize this token.
    pub goal: LexicalGoal,
}

/// Evidence for an explicit or automatic semicolon boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Asi {
    /// Source contained a semicolon.
    Explicit(Span),
    /// A line terminator caused insertion.
    LineTerminator(Span),
    /// A closing brace caused insertion.
    ClosingBrace(Span),
    /// End of input caused insertion.
    EndOfInput(Span),
}

/// Neutral concrete-tree node category, stable for downstream extensions.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NodeKind {
    /// Script root.
    Script,
    /// Module root.
    Module,
    /// Statement list.
    StatementList,
    /// A declaration.
    Declaration,
    /// A statement.
    Statement,
    /// Function declaration or expression.
    Function,
    /// Class declaration or expression.
    Class,
    /// Import declaration.
    Import,
    /// Export declaration.
    Export,
    /// Expression region grouped through shared Pratt precedence.
    Expression,
    /// Delimited or computed region.
    Group,
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
    /// Semicolon boundary, when applicable.
    pub asi: Option<Asi>,
}

/// Source identity attachable by JavaScript or downstream syntax extensions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Origin {
    /// Caller-owned source identity.
    pub source: String,
    /// Source byte range.
    pub span: Span,
    /// Optional parent origin, preserving transformation chains.
    pub parent: Option<Box<Origin>>,
}

/// Complete lossless syntax tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxTree {
    source: String,
    /// Selected root goal.
    pub goal: Goal,
    /// Lossless tokens, including trivia.
    pub tokens: Vec<Token>,
    /// Structural root.
    pub root: Node,
}
impl SyntaxTree {
    /// Returns the exact admitted input.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Re-emits the input byte-for-byte.
    #[must_use]
    pub fn preserve_source(&self) -> String {
        self.source.clone()
    }
    pub(crate) fn new(source: &str, goal: Goal, tokens: Vec<Token>, root: Node) -> Self {
        Self {
            source: source.to_owned(),
            goal,
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
    /// Maximum emitted tokens.
    pub max_tokens: usize,
    /// Maximum delimiter/template/tree nesting.
    pub max_nesting: usize,
    /// Maximum physical lines.
    pub max_lines: usize,
    /// Maximum nodes.
    pub max_nodes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_tokens: 1_000_000,
            max_nesting: 256,
            max_lines: 250_000,
            max_nodes: 1_000_000,
        }
    }
}

/// Stable diagnostic category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    /// Configured resource bound crossed.
    ResourceLimit,
    /// Invalid source character.
    InvalidCharacter,
    /// Unterminated literal or comment.
    UnterminatedLiteral,
    /// Unmatched or crossed delimiter.
    UnmatchedDelimiter,
    /// Grammar violation.
    InvalidSyntax,
    /// Static-semantics early error.
    EarlyError,
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
    /// Zero-based scalar column.
    pub column: usize,
    /// Stable detail.
    pub message: String,
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}
impl std::error::Error for Diagnostic {}
