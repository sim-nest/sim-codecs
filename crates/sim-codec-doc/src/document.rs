//! The document model and chunking. Defines `DocValue`/`DocBlock`/`DocChunk`
//! and their `Expr` projection, parses document text into blocks
//! (`decode_document`), and splits documents into provenance-preserving chunks
//! (`chunk`, `ChunkOp`).

use sim_kernel::{Error, Expr, NumberLiteral, Result, Symbol};

/// A decoded document: its full source text, detected format, and the ordered
/// [`DocBlock`]s parsed from it.
///
/// Block offsets index into [`text`](DocValue::text), so the document carries
/// provenance for every span it exposes and round-trips back to the original
/// source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocValue {
    /// The complete, unmodified source text of the document.
    pub text: String,
    /// The detected source format (`DocFormat::Markdown` when any heading was
    /// seen, otherwise `DocFormat::Text`).
    pub format: DocFormat,
    /// The ordered blocks parsed from [`text`](DocValue::text).
    pub blocks: Vec<DocBlock>,
}

/// The detected surface format of a decoded document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocFormat {
    /// Plain text: no Markdown headings were detected.
    Text,
    /// Markdown: at least one `#`-prefixed heading was detected.
    Markdown,
}

/// One parsed span of a document, carrying its byte offsets and the heading
/// path in effect where it appears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocBlock {
    /// Whether this block is a heading or a paragraph.
    pub kind: DocBlockKind,
    /// The block's text: the heading title for headings, the joined paragraph
    /// lines for paragraphs.
    pub text: String,
    /// Inclusive start byte offset of the block within the source text.
    pub start: usize,
    /// Exclusive end byte offset of the block within the source text.
    pub end: usize,
    /// The chain of enclosing heading titles, outermost first.
    pub heading_path: Vec<String>,
    /// The heading depth (1-6) for headings; `None` for paragraphs.
    pub level: Option<usize>,
}

/// The category of a [`DocBlock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocBlockKind {
    /// A Markdown heading line.
    Heading,
    /// A run of non-blank, non-heading lines.
    Paragraph,
}

/// A provenance-preserving slice of a document produced by [`chunk`].
///
/// Like [`DocBlock`], the byte offsets index into the originating document's
/// source text, so a chunk can always be traced back to its origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocChunk {
    /// The chunk's text, copied from the source span.
    pub text: String,
    /// Inclusive start byte offset of the chunk within the source text.
    pub start: usize,
    /// Exclusive end byte offset of the chunk within the source text.
    pub end: usize,
    /// The heading path in effect for the source span, outermost first.
    pub heading_path: Vec<String>,
}

/// A chunking strategy selecting how [`chunk`] splits a [`DocValue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkOp {
    /// Split the whole source into fixed-size windows of at most the given
    /// number of bytes, respecting character boundaries.
    Fixed(usize),
    /// Emit one chunk per paragraph, further splitting any paragraph longer
    /// than `max` bytes into fixed windows.
    Recursive {
        /// The maximum chunk size in bytes before a paragraph is split.
        max: usize,
    },
    /// Emit one chunk per paragraph, each carrying its heading path.
    Heading,
}

/// Parse document `text` into a [`DocValue`], recording per-block byte offsets
/// and heading paths.
///
/// Headings are `#`-prefixed lines (levels 1-6); everything else groups into
/// paragraphs split on blank lines. The format is reported as
/// `DocFormat::Markdown` when any heading is present, otherwise
/// `DocFormat::Text`. The returned value's [`text`](DocValue::text) is the
/// input verbatim.
///
/// # Examples
///
/// ```
/// use sim_codec_doc::{DocBlockKind, DocFormat, decode_document};
///
/// let doc = decode_document("# Guide\n\nAlpha beta.\n");
/// assert_eq!(doc.format, DocFormat::Markdown);
/// assert_eq!(doc.blocks.len(), 2);
/// assert_eq!(doc.blocks[0].kind, DocBlockKind::Heading);
/// assert_eq!(doc.blocks[1].text, "Alpha beta.");
/// assert_eq!(doc.blocks[1].heading_path, vec!["Guide".to_owned()]);
/// ```
pub fn decode_document(text: &str) -> DocValue {
    let mut blocks = Vec::new();
    let mut headings: Vec<String> = Vec::new();
    let mut paragraph: Option<(usize, usize, Vec<String>)> = None;
    let mut saw_heading = false;

    for (line_start, line_end, line) in line_segments(text) {
        if let Some((level, title)) = heading(line) {
            flush_paragraph(text, &mut blocks, &mut paragraph);
            saw_heading = true;
            while headings.len() >= level {
                headings.pop();
            }
            headings.push(title.clone());
            blocks.push(DocBlock {
                kind: DocBlockKind::Heading,
                text: title,
                start: line_start,
                end: line_end,
                heading_path: headings.clone(),
                level: Some(level),
            });
        } else if line.trim().is_empty() {
            flush_paragraph(text, &mut blocks, &mut paragraph);
        } else if let Some((_, end, _)) = &mut paragraph {
            *end = line_end;
        } else {
            paragraph = Some((line_start, line_end, headings.clone()));
        }
    }

    flush_paragraph(text, &mut blocks, &mut paragraph);

    DocValue {
        text: text.to_owned(),
        format: if saw_heading {
            DocFormat::Markdown
        } else {
            DocFormat::Text
        },
        blocks,
    }
}

/// Split a decoded `doc` into [`DocChunk`]s according to `op`.
///
/// Every chunk preserves its source byte offsets and heading path, so the
/// resulting chunks round-trip through the general-purpose codecs as ordinary
/// document slices. See [`ChunkOp`] for the available strategies.
///
/// # Examples
///
/// ```
/// use sim_codec_doc::{ChunkOp, chunk, decode_document};
///
/// let doc = decode_document("abcdef");
/// let chunks = chunk(&doc, ChunkOp::Fixed(2));
/// assert_eq!(chunks.len(), 3);
/// assert_eq!(chunks[0].text, "ab");
/// assert_eq!((chunks[0].start, chunks[0].end), (0, 2));
/// ```
pub fn chunk(doc: &DocValue, op: ChunkOp) -> Vec<DocChunk> {
    match op {
        ChunkOp::Fixed(max) => fixed_range(&doc.text, 0, doc.text.len(), max, Vec::new()),
        ChunkOp::Recursive { max } => recursive_chunks(doc, max),
        ChunkOp::Heading => heading_chunks(doc),
    }
}

impl DocValue {
    /// Project this document into its `Expr` map form (`kind: doc`, with
    /// `format`, `text`, and a list of block maps).
    pub fn as_expr(&self) -> Expr {
        Expr::Map(vec![
            key("kind", Expr::Symbol(Symbol::new("doc"))),
            key("format", Expr::Symbol(Symbol::new(self.format.name()))),
            key("text", Expr::String(self.text.clone())),
            key(
                "blocks",
                Expr::List(self.blocks.iter().map(DocBlock::as_expr).collect()),
            ),
        ])
    }

    /// Reconstruct a [`DocValue`] from an `Expr`, accepting either a raw string
    /// (decoded as document text) or a `kind: doc` map (decoded from its `text`
    /// field). Fails closed on any other expression shape.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        match expr {
            Expr::String(text) => Ok(decode_document(text)),
            Expr::Map(entries) => {
                let kind = map_field(entries, "kind")
                    .ok_or_else(|| Error::Eval("document value requires kind field".to_owned()))?;
                let Expr::Symbol(symbol) = kind else {
                    return Err(Error::Eval("document kind must be a symbol".to_owned()));
                };
                if symbol.name.as_ref() != "doc" {
                    return Err(Error::Eval("document kind must be doc".to_owned()));
                }
                let text = map_string(entries, "text")?;
                Ok(decode_document(text))
            }
            _ => Err(Error::TypeMismatch {
                expected: "document value",
                found: "non-document",
            }),
        }
    }
}

impl DocFormat {
    fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
        }
    }
}

impl DocBlock {
    fn as_expr(&self) -> Expr {
        let mut entries = vec![
            key("kind", Expr::Symbol(Symbol::new(self.kind.name()))),
            key("text", Expr::String(self.text.clone())),
            key("start", number(self.start)),
            key("end", number(self.end)),
            key("heading_path", string_list(&self.heading_path)),
        ];
        if let Some(level) = self.level {
            entries.push(key("level", number(level)));
        }
        Expr::Map(entries)
    }
}

impl DocBlockKind {
    fn name(self) -> &'static str {
        match self {
            Self::Heading => "heading",
            Self::Paragraph => "paragraph",
        }
    }
}

impl DocChunk {
    /// Project this chunk into its `Expr` map form (`kind: doc-chunk`, with
    /// `text`, `start`, `end`, and `heading_path`).
    pub fn as_expr(&self) -> Expr {
        Expr::Map(vec![
            key("kind", Expr::Symbol(Symbol::new("doc-chunk"))),
            key("text", Expr::String(self.text.clone())),
            key("start", number(self.start)),
            key("end", number(self.end)),
            key("heading_path", string_list(&self.heading_path)),
        ])
    }
}

fn recursive_chunks(doc: &DocValue, max: usize) -> Vec<DocChunk> {
    if max == 0 {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    for block in doc
        .blocks
        .iter()
        .filter(|block| block.kind == DocBlockKind::Paragraph)
    {
        if block.end.saturating_sub(block.start) <= max {
            chunks.push(chunk_for_range(
                &doc.text,
                block.start,
                block.end,
                block.heading_path.clone(),
            ));
        } else {
            chunks.extend(fixed_range(
                &doc.text,
                block.start,
                block.end,
                max,
                block.heading_path.clone(),
            ));
        }
    }
    if chunks.is_empty() && !doc.text.is_empty() {
        chunks.extend(fixed_range(&doc.text, 0, doc.text.len(), max, Vec::new()));
    }
    chunks
}

fn heading_chunks(doc: &DocValue) -> Vec<DocChunk> {
    let chunks = doc
        .blocks
        .iter()
        .filter(|block| block.kind == DocBlockKind::Paragraph)
        .map(|block| {
            chunk_for_range(
                &doc.text,
                block.start,
                block.end,
                block.heading_path.clone(),
            )
        })
        .collect::<Vec<_>>();
    if chunks.is_empty() && !doc.text.is_empty() {
        vec![chunk_for_range(&doc.text, 0, doc.text.len(), Vec::new())]
    } else {
        chunks
    }
}

fn fixed_range(
    text: &str,
    start: usize,
    end: usize,
    max: usize,
    heading_path: Vec<String>,
) -> Vec<DocChunk> {
    if max == 0 || start >= end {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let next = char_boundary_at_or_before(text, cursor.saturating_add(max).min(end), cursor);
        let next = if next == cursor {
            next_char_boundary_after(text, cursor).min(end)
        } else {
            next
        };
        chunks.push(chunk_for_range(text, cursor, next, heading_path.clone()));
        cursor = next;
    }
    chunks
}

fn chunk_for_range(text: &str, start: usize, end: usize, heading_path: Vec<String>) -> DocChunk {
    DocChunk {
        text: text[start..end].to_owned(),
        start,
        end,
        heading_path,
    }
}

fn char_boundary_at_or_before(text: &str, mut index: usize, floor: usize) -> usize {
    while index > floor && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_char_boundary_after(text: &str, index: usize) -> usize {
    text[index..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| index + offset)
        .unwrap_or(text.len())
}

fn flush_paragraph(
    source: &str,
    blocks: &mut Vec<DocBlock>,
    paragraph: &mut Option<(usize, usize, Vec<String>)>,
) {
    let Some((start, end, heading_path)) = paragraph.take() else {
        return;
    };
    blocks.push(DocBlock {
        kind: DocBlockKind::Paragraph,
        text: source[start..end].to_owned(),
        start,
        end,
        heading_path,
        level: None,
    });
}

fn line_segments(source: &str) -> Vec<(usize, usize, &str)> {
    let mut segments = Vec::new();
    let mut offset = 0;
    for segment in source.split_inclusive('\n') {
        let start = offset;
        offset += segment.len();
        let mut end = offset;
        if segment.ends_with('\n') {
            end -= 1;
            if end > start && source.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
        }
        segments.push((start, end, &source[start..end]));
    }
    segments
}

fn heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let title = rest.trim();
    (!title.is_empty()).then(|| (level, title.to_owned()))
}

fn key(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}

fn number(value: usize) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}

fn string_list(values: &[String]) -> Expr {
    Expr::List(values.iter().cloned().map(Expr::String).collect())
}

use sim_value::access::entry_field as map_field;

fn map_string<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Result<&'a str> {
    match map_field(entries, key) {
        Some(Expr::String(value)) => Ok(value),
        Some(_) => Err(Error::Eval(format!(
            "document {key} field must be a string"
        ))),
        None => Err(Error::Eval(format!("document value requires {key} field"))),
    }
}
