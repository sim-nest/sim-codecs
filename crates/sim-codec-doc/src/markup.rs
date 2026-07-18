//! Shared semantic markup document IR and `Expr` projection.

use std::collections::BTreeMap;

use sim_kernel::{Error, Expr, Result};
use sim_value::build::{entry, list, map, sym, text, uint};

mod decode;
mod expr;
mod model;

pub use decode::decode_markup_doc;
use expr::*;
pub use model::{
    BackendId, Inline, MarkupBlock, MarkupDoc, MathSource, SourceDoc, Span, SpanState,
};

impl MarkupDoc {
    /// Project this markup document into ordinary SIM data.
    pub fn as_expr(&self) -> Expr {
        let mut entries = vec![
            entry("kind", sym("markup-doc")),
            entry(
                "blocks",
                list(self.blocks.iter().map(MarkupBlock::as_expr).collect()),
            ),
            entry("attrs", attrs_expr(&self.attrs)),
        ];
        if let Some(title) = &self.title {
            entries.push(entry("title", text(title)));
        }
        if let Some(source) = &self.source {
            entries.push(entry("source", source.as_expr()));
        }
        Expr::Map(entries)
    }

    /// Reconstruct a markup document from ordinary SIM data.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let entries = map_entries(expr, "markup document")?;
        require_kind(entries, "markup-doc", "markup document")?;
        let title = optional_string(entries, "title")?.map(str::to_owned);
        let blocks = required_list(entries, "blocks", "markup document")?
            .iter()
            .map(MarkupBlock::from_expr)
            .collect::<Result<Vec<_>>>()?;
        let attrs = match field(entries, "attrs") {
            Some(Expr::Map(attrs)) => attrs_from_entries(attrs.as_slice())?,
            Some(_) => return Err(Error::Eval("markup attrs must be a map".to_owned())),
            None => BTreeMap::new(),
        };
        let source = match field(entries, "source") {
            Some(expr) => Some(SourceDoc::from_expr(expr)?),
            None => None,
        };
        Ok(Self {
            title,
            blocks,
            attrs,
            source,
        })
    }

    /// Render this document to a deterministic source string.
    ///
    /// When verbatim source is present, it wins. Documents constructed directly
    /// from semantic blocks use a small Markdown-like writer.
    pub fn to_source_text(&self) -> String {
        if let Some(source) = &self.source {
            return source.text.clone();
        }
        let mut out = String::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                out.push_str("\n\n");
            }
            block.write_source(&mut out);
        }
        out
    }
}

impl SourceDoc {
    fn as_expr(&self) -> Expr {
        map(vec![
            ("backend", text(&self.backend.0)),
            ("text", text(&self.text)),
        ])
    }

    fn from_expr(expr: &Expr) -> Result<Self> {
        let entries = map_entries(expr, "markup source")?;
        Ok(Self {
            backend: BackendId(required_string(entries, "backend", "markup source")?.to_owned()),
            text: required_string(entries, "text", "markup source")?.to_owned(),
        })
    }
}

impl Span {
    fn as_expr(&self) -> Expr {
        map(vec![
            ("start", uint(self.start as u64)),
            ("end", uint(self.end as u64)),
            ("state", sym(self.state.as_str())),
        ])
    }

    fn from_expr(expr: &Expr) -> Result<Self> {
        let entries = map_entries(expr, "span")?;
        Ok(Self {
            start: required_usize(entries, "start", "span")?,
            end: required_usize(entries, "end", "span")?,
            state: match field(entries, "state") {
                Some(value) => SpanState::from_expr(value)?,
                None => SpanState::Preserved,
            },
        })
    }
}

impl SpanState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Preserved => "preserved",
            Self::Dirty => "dirty",
        }
    }

    fn from_expr(expr: &Expr) -> Result<Self> {
        match expr {
            Expr::Symbol(symbol) if symbol.namespace.is_none() => match symbol.name.as_ref() {
                "preserved" => Ok(Self::Preserved),
                "dirty" => Ok(Self::Dirty),
                other => Err(Error::Eval(format!("unknown span state {other}"))),
            },
            Expr::String(value) => match value.as_str() {
                "preserved" => Ok(Self::Preserved),
                "dirty" => Ok(Self::Dirty),
                other => Err(Error::Eval(format!("unknown span state {other}"))),
            },
            _ => Err(Error::Eval("span state must be a symbol".to_owned())),
        }
    }
}

impl MathSource {
    fn as_expr(&self) -> Expr {
        map(vec![
            ("notation", text(&self.notation)),
            ("text", text(&self.text)),
        ])
    }

    fn from_expr(expr: &Expr) -> Result<Self> {
        let entries = map_entries(expr, "math source")?;
        Ok(Self {
            notation: required_string(entries, "notation", "math source")?.to_owned(),
            text: required_string(entries, "text", "math source")?.to_owned(),
        })
    }
}

impl MarkupBlock {
    /// Project this block into ordinary SIM data.
    pub fn as_expr(&self) -> Expr {
        match self {
            Self::Heading {
                level,
                text: heading,
                id,
                span,
            } => {
                let mut entries = vec![
                    entry("kind", sym("heading")),
                    entry("level", uint(u64::from(*level))),
                    entry("text", inline_list(heading)),
                ];
                push_optional_string(&mut entries, "id", id);
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::Paragraph { content, span } => {
                let mut entries = vec![
                    entry("kind", sym("paragraph")),
                    entry("content", inline_list(content)),
                ];
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::CodeBlock { lang, code, span } => {
                let mut entries = vec![entry("kind", sym("code-block")), entry("code", text(code))];
                push_optional_string(&mut entries, "lang", lang);
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::MathBlock { source, span } => {
                let mut entries = vec![
                    entry("kind", sym("math-block")),
                    entry("source", source.as_expr()),
                ];
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::Quote { blocks, span } => {
                let mut entries = vec![
                    entry("kind", sym("quote")),
                    entry("blocks", block_list(blocks)),
                ];
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::List {
                ordered,
                items,
                span,
            } => {
                let mut entries = vec![
                    entry("kind", sym("list")),
                    entry("ordered", Expr::Bool(*ordered)),
                    entry(
                        "items",
                        list(items.iter().map(|item| block_list(item)).collect()),
                    ),
                ];
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::Table { header, rows, span } => {
                let mut entries = vec![
                    entry("kind", sym("table")),
                    entry(
                        "header",
                        list(header.iter().map(|cell| inline_list(cell)).collect()),
                    ),
                    entry(
                        "rows",
                        list(
                            rows.iter()
                                .map(|row| list(row.iter().map(|cell| inline_list(cell)).collect()))
                                .collect(),
                        ),
                    ),
                ];
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::Figure { src, caption, span } => {
                let mut entries = vec![
                    entry("kind", sym("figure")),
                    entry("src", text(src)),
                    entry("caption", inline_list(caption)),
                ];
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
            Self::Raw {
                backend,
                text: raw,
                span,
            } => {
                let mut entries = vec![
                    entry("kind", sym("raw")),
                    entry("backend", text(&backend.0)),
                    entry("text", text(raw)),
                ];
                push_optional_span(&mut entries, span);
                Expr::Map(entries)
            }
        }
    }

    /// Reconstruct a block from ordinary SIM data.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let entries = map_entries(expr, "markup block")?;
        match required_kind(entries, "markup block")?.as_str() {
            "heading" => Ok(Self::Heading {
                level: required_u8(entries, "level", "heading")?,
                text: inline_vec(required_list(entries, "text", "heading")?)?,
                id: optional_string(entries, "id")?.map(str::to_owned),
                span: optional_span(entries)?,
            }),
            "paragraph" => Ok(Self::Paragraph {
                content: inline_vec(required_list(entries, "content", "paragraph")?)?,
                span: optional_span(entries)?,
            }),
            "code-block" => Ok(Self::CodeBlock {
                lang: optional_string(entries, "lang")?.map(str::to_owned),
                code: required_string(entries, "code", "code block")?.to_owned(),
                span: optional_span(entries)?,
            }),
            "math-block" => Ok(Self::MathBlock {
                source: MathSource::from_expr(required_field(entries, "source", "math block")?)?,
                span: optional_span(entries)?,
            }),
            "quote" => Ok(Self::Quote {
                blocks: block_vec(required_list(entries, "blocks", "quote")?)?,
                span: optional_span(entries)?,
            }),
            "list" => {
                let items = required_list(entries, "items", "list")?
                    .iter()
                    .map(|item| block_vec(as_list(item, "list item")?))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::List {
                    ordered: required_bool(entries, "ordered", "list")?,
                    items,
                    span: optional_span(entries)?,
                })
            }
            "table" => {
                let header = required_list(entries, "header", "table")?
                    .iter()
                    .map(|cell| inline_vec(as_list(cell, "table header cell")?))
                    .collect::<Result<Vec<_>>>()?;
                let rows = required_list(entries, "rows", "table")?
                    .iter()
                    .map(|row| {
                        as_list(row, "table row")?
                            .iter()
                            .map(|cell| inline_vec(as_list(cell, "table cell")?))
                            .collect::<Result<Vec<_>>>()
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::Table {
                    header,
                    rows,
                    span: optional_span(entries)?,
                })
            }
            "figure" => Ok(Self::Figure {
                src: required_string(entries, "src", "figure")?.to_owned(),
                caption: inline_vec(required_list(entries, "caption", "figure")?)?,
                span: optional_span(entries)?,
            }),
            "raw" => Ok(Self::Raw {
                backend: BackendId(required_string(entries, "backend", "raw block")?.to_owned()),
                text: required_string(entries, "text", "raw block")?.to_owned(),
                span: optional_span(entries)?,
            }),
            other => Err(Error::Eval(format!("unknown markup block kind {other}"))),
        }
    }

    fn write_source(&self, out: &mut String) {
        match self {
            Self::Heading { level, text, .. } => {
                out.push_str(&"#".repeat(usize::from(*level).max(1)));
                out.push(' ');
                write_inlines(out, text);
            }
            Self::Paragraph { content, .. } => write_inlines(out, content),
            Self::CodeBlock { lang, code, .. } => {
                out.push_str("```");
                if let Some(lang) = lang {
                    out.push_str(lang);
                }
                out.push('\n');
                out.push_str(code);
                if !code.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```");
            }
            Self::MathBlock { source, .. } => {
                out.push_str("$$\n");
                out.push_str(&source.text);
                out.push_str("\n$$");
            }
            Self::Quote { blocks, .. } => {
                let text = blocks_to_source(blocks);
                for (index, line) in text.lines().enumerate() {
                    if index > 0 {
                        out.push('\n');
                    }
                    out.push_str("> ");
                    out.push_str(line);
                }
            }
            Self::List { ordered, items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push('\n');
                    }
                    if *ordered {
                        out.push_str(&format!("{}. ", index + 1));
                    } else {
                        out.push_str("- ");
                    }
                    out.push_str(&blocks_to_source(item).replace('\n', "\n  "));
                }
            }
            Self::Table { header, rows, .. } => {
                write_table_row(out, header);
                out.push('\n');
                write_table_separator(out, header.len());
                for row in rows {
                    out.push('\n');
                    write_table_row(out, row);
                }
            }
            Self::Figure { src, caption, .. } => {
                out.push_str("![");
                write_inlines(out, caption);
                out.push_str("](");
                out.push_str(src);
                out.push(')');
            }
            Self::Raw { text, .. } => out.push_str(text),
        }
    }
}

impl Inline {
    fn as_expr(&self) -> Expr {
        match self {
            Self::Text(value) => map(vec![("kind", sym("text")), ("text", text(value))]),
            Self::Emph(items) => map(vec![("kind", sym("emph")), ("content", inline_list(items))]),
            Self::Strong(items) => map(vec![
                ("kind", sym("strong")),
                ("content", inline_list(items)),
            ]),
            Self::Code(value) => map(vec![("kind", sym("code")), ("text", text(value))]),
            Self::Link { label, target } => map(vec![
                ("kind", sym("link")),
                ("label", inline_list(label)),
                ("target", text(target)),
            ]),
            Self::Math(source) => map(vec![("kind", sym("math")), ("source", source.as_expr())]),
            Self::Raw { backend, text: raw } => map(vec![
                ("kind", sym("raw")),
                ("backend", text(&backend.0)),
                ("text", text(raw)),
            ]),
        }
    }

    fn from_expr(expr: &Expr) -> Result<Self> {
        if let Expr::String(value) = expr {
            return Ok(Self::Text(value.clone()));
        }
        let entries = map_entries(expr, "inline")?;
        match required_kind(entries, "inline")?.as_str() {
            "text" => Ok(Self::Text(
                required_string(entries, "text", "inline")?.to_owned(),
            )),
            "emph" => Ok(Self::Emph(inline_vec(required_list(
                entries, "content", "inline",
            )?)?)),
            "strong" => Ok(Self::Strong(inline_vec(required_list(
                entries, "content", "inline",
            )?)?)),
            "code" => Ok(Self::Code(
                required_string(entries, "text", "inline")?.to_owned(),
            )),
            "link" => Ok(Self::Link {
                label: inline_vec(required_list(entries, "label", "inline")?)?,
                target: required_string(entries, "target", "inline")?.to_owned(),
            }),
            "math" => Ok(Self::Math(MathSource::from_expr(required_field(
                entries, "source", "inline",
            )?)?)),
            "raw" => Ok(Self::Raw {
                backend: BackendId(required_string(entries, "backend", "inline")?.to_owned()),
                text: required_string(entries, "text", "inline")?.to_owned(),
            }),
            other => Err(Error::Eval(format!("unknown inline kind {other}"))),
        }
    }
}
