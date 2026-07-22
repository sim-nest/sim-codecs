use std::collections::BTreeMap;
use std::fmt;

use sim_kernel::Expr;

/// A semantic markup document independent of its concrete source backend.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkupDoc {
    /// Optional document title.
    pub title: Option<String>,
    /// Semantic block sequence.
    pub blocks: Vec<MarkupBlock>,
    /// Open document attributes carried as ordinary SIM expressions.
    pub attrs: BTreeMap<String, Expr>,
    /// Original source text, when a backend can preserve it.
    pub source: Option<SourceDoc>,
}

/// A concrete source document preserved alongside the semantic IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDoc {
    /// Backend that produced the source text.
    pub backend: BackendId,
    /// Verbatim source text.
    pub text: String,
}

/// A backend identifier such as `markdown`, `typst`, `asciidoc`, or `latex`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BackendId(
    /// Backend id text.
    pub String,
);

impl BackendId {
    /// Create a backend id from stable id text.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the backend id text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BackendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A source span in byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
    /// Whether the semantic node still matches the original source span.
    pub state: SpanState,
}

/// Source-span freshness after reversible document edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpanState {
    /// The span still points at untouched source bytes.
    Preserved,
    /// The span's source bytes must be regenerated.
    Dirty,
}

/// Markup math source with a notation label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathSource {
    /// Math notation, such as `tex`, `typst`, `asciimath`, or `unknown`.
    pub notation: String,
    /// Source text for the math expression.
    pub text: String,
}

/// A semantic block in a markup document.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkupBlock {
    /// A heading with inline text.
    Heading {
        /// Heading level, normally 1 through 6.
        level: u8,
        /// Inline heading content.
        text: Vec<Inline>,
        /// Optional stable heading id.
        id: Option<String>,
        /// Optional source span.
        span: Option<Span>,
    },
    /// A paragraph of inline content.
    Paragraph {
        /// Inline paragraph content.
        content: Vec<Inline>,
        /// Optional source span.
        span: Option<Span>,
    },
    /// A fenced or literal code block.
    CodeBlock {
        /// Optional language tag.
        lang: Option<String>,
        /// Code text.
        code: String,
        /// Optional source span.
        span: Option<Span>,
    },
    /// A display math block.
    MathBlock {
        /// Math source.
        source: MathSource,
        /// Optional source span.
        span: Option<Span>,
    },
    /// A block quote.
    Quote {
        /// Quoted blocks.
        blocks: Vec<MarkupBlock>,
        /// Optional source span.
        span: Option<Span>,
    },
    /// An ordered or unordered list.
    List {
        /// Whether the list is ordered.
        ordered: bool,
        /// List items, each item containing block content.
        items: Vec<Vec<MarkupBlock>>,
        /// Optional source span.
        span: Option<Span>,
    },
    /// A simple table.
    Table {
        /// Header cells.
        header: Vec<Vec<Inline>>,
        /// Row cells.
        rows: Vec<Vec<Vec<Inline>>>,
        /// Optional source span.
        span: Option<Span>,
    },
    /// A figure with source and caption.
    Figure {
        /// Image/media source.
        src: String,
        /// Figure caption.
        caption: Vec<Inline>,
        /// Optional source span.
        span: Option<Span>,
    },
    /// Backend-specific raw text.
    Raw {
        /// Backend that owns the raw text.
        backend: BackendId,
        /// Raw source text.
        text: String,
        /// Optional source span.
        span: Option<Span>,
    },
}

/// Inline content in a markup document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    /// Plain text.
    Text(String),
    /// Emphasized inline content.
    Emph(Vec<Inline>),
    /// Strong inline content.
    Strong(Vec<Inline>),
    /// Inline code.
    Code(String),
    /// Link with label and target.
    Link {
        /// Link label.
        label: Vec<Inline>,
        /// Link target.
        target: String,
    },
    /// Inline math.
    Math(MathSource),
    /// Backend-specific raw inline text.
    Raw {
        /// Backend that owns the raw text.
        backend: BackendId,
        /// Raw source text.
        text: String,
    },
}
