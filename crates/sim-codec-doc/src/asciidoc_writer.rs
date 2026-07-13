//! AsciiDoc writer used by the safe AsciiDoc backend.

use crate::backend::{MarkupEncodeOptions, MarkupFidelity, MarkupLoss};
use crate::markup::{Inline, MarkupBlock, MarkupDoc};

use super::asciidoc_support::{asciidoc_id, inline_plain_text};

/// Narrow AsciiDoc emitter for semantic markup documents.
pub(super) struct AsciiDocEncoder {
    preserve_raw: bool,
    fidelity: MarkupFidelity,
}

impl AsciiDocEncoder {
    /// Creates an AsciiDoc encoder with the requested raw-fragment policy.
    pub(super) fn new(opts: &MarkupEncodeOptions) -> Self {
        Self {
            preserve_raw: opts.preserve_raw,
            fidelity: MarkupFidelity::exact(asciidoc_id()),
        }
    }

    /// Renders the document as AsciiDoc source.
    pub(super) fn write_doc(&mut self, doc: &MarkupDoc) -> String {
        let skip_title_heading = doc
            .title
            .as_ref()
            .is_some_and(|title| first_heading_matches_title(&doc.blocks, title));
        let mut out = String::new();
        if let Some(title) = &doc.title {
            out.push_str("= ");
            write_text(title, &mut out);
            let needs_gap = if skip_title_heading {
                doc.blocks
                    .iter()
                    .any(|block| !is_matching_title(block, title))
            } else {
                !doc.blocks.is_empty()
            };
            if needs_gap {
                out.push_str("\n\n");
            }
        }
        let blocks = if skip_title_heading {
            &doc.blocks[1..]
        } else {
            &doc.blocks[..]
        };
        out.push_str(&self.render_blocks(blocks));
        out
    }

    /// Returns the fidelity report collected so far.
    pub(super) fn fidelity(&self) -> &MarkupFidelity {
        &self.fidelity
    }

    /// Finishes the encoder and returns its fidelity report.
    pub(super) fn into_fidelity(self) -> MarkupFidelity {
        self.fidelity
    }

    fn render_blocks(&mut self, blocks: &[MarkupBlock]) -> String {
        let mut out = String::new();
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 {
                out.push_str("\n\n");
            }
            self.write_block(block, &mut out);
        }
        out
    }

    fn write_block(&mut self, block: &MarkupBlock, out: &mut String) {
        match block {
            MarkupBlock::Heading { level, text, .. } => {
                out.push_str(&"=".repeat(usize::from(*level).max(1)));
                out.push(' ');
                self.write_inlines(text, out);
            }
            MarkupBlock::Paragraph { content, .. } => self.write_inlines(content, out),
            MarkupBlock::CodeBlock { lang, code, .. } => self.write_code_block(lang, code, out),
            MarkupBlock::MathBlock { source, .. } => self.write_math_block(&source.text, out),
            MarkupBlock::Quote { blocks, .. } => self.write_quote(blocks, out),
            MarkupBlock::List { ordered, items, .. } => self.write_list(*ordered, items, out),
            MarkupBlock::Table { header, rows, .. } => self.write_table(header, rows, out),
            MarkupBlock::Figure { src, caption, .. } => self.write_figure(src, caption, out),
            MarkupBlock::Raw { backend, text, .. } if backend == &asciidoc_id() => {
                self.write_raw(text, "raw-block", out);
            }
            MarkupBlock::Raw { text, .. } => self.write_raw(text, "raw-block", out),
        }
    }

    fn write_code_block(&mut self, lang: &Option<String>, code: &str, out: &mut String) {
        if let Some(lang) = lang {
            out.push_str("[source,");
            write_attr(lang, out);
            out.push_str("]\n");
        }
        out.push_str("----\n");
        out.push_str(code);
        if !code.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("----");
    }

    fn write_math_block(&mut self, text: &str, out: &mut String) {
        out.push_str("[stem]\n++++\n");
        out.push_str(text.trim());
        out.push_str("\n++++");
    }

    fn write_quote(&mut self, blocks: &[MarkupBlock], out: &mut String) {
        out.push_str("____\n");
        out.push_str(&self.render_blocks(blocks));
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("____");
    }

    fn write_list(&mut self, ordered: bool, items: &[Vec<MarkupBlock>], out: &mut String) {
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(if ordered { ". " } else { "* " });
            let rendered = self.render_blocks(item);
            out.push_str(&rendered.replace('\n', "\n  "));
        }
    }

    fn write_table(&mut self, header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
        if !header.is_empty() {
            out.push_str("[options=\"header\"]\n");
        }
        out.push_str("|===\n");
        if !header.is_empty() {
            self.write_table_row(header, out);
        }
        for row in rows {
            self.write_table_row(row, out);
        }
        out.push_str("|===");
    }

    fn write_table_row(&mut self, row: &[Vec<Inline>], out: &mut String) {
        for cell in row {
            out.push('|');
            self.write_inlines(cell, out);
            out.push(' ');
        }
        out.push('\n');
    }

    fn write_figure(&mut self, src: &str, caption: &[Inline], out: &mut String) {
        out.push_str("image::");
        write_target(src, out);
        out.push('[');
        self.write_inlines(caption, out);
        out.push(']');
    }

    fn write_inlines(&mut self, items: &[Inline], out: &mut String) {
        for item in items {
            match item {
                Inline::Text(value) => write_text(value, out),
                Inline::Emph(children) => {
                    out.push('_');
                    self.write_inlines(children, out);
                    out.push('_');
                }
                Inline::Strong(children) => {
                    out.push('*');
                    self.write_inlines(children, out);
                    out.push('*');
                }
                Inline::Code(value) => {
                    out.push('`');
                    out.push_str(value);
                    out.push('`');
                }
                Inline::Link { label, target } => {
                    out.push_str("link:");
                    write_target(target, out);
                    out.push('[');
                    self.write_inlines(label, out);
                    out.push(']');
                }
                Inline::Math(source) => {
                    out.push_str("stem:[");
                    out.push_str(source.text.trim());
                    out.push(']');
                }
                Inline::Raw { text, .. } => self.write_raw(text, "raw-inline", out),
            }
        }
    }

    fn write_raw(&mut self, text: &str, path: &str, out: &mut String) {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(text.to_owned());
            out.push_str(text);
        } else {
            self.fidelity.dropped.push(MarkupLoss {
                path: path.to_owned(),
                reason: "raw asciidoc fragment omitted".to_owned(),
            });
        }
    }
}

fn first_heading_matches_title(blocks: &[MarkupBlock], title: &str) -> bool {
    blocks
        .first()
        .is_some_and(|block| is_matching_title(block, title))
}

fn is_matching_title(block: &MarkupBlock, title: &str) -> bool {
    matches!(
        block,
        MarkupBlock::Heading { level: 1, text, .. } if inline_plain_text(text) == title
    )
}

fn write_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        if matches!(ch, '*' | '_' | '`' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
}

fn write_target(text: &str, out: &mut String) {
    for ch in text.chars() {
        if matches!(ch, '[' | ']' | '\\' | ' ') {
            out.push('\\');
        }
        out.push(ch);
    }
}

fn write_attr(text: &str, out: &mut String) {
    for ch in text.chars() {
        if matches!(ch, ',' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
}
