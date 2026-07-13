//! Typst writer used by the safe Typst backend.

use crate::backend::{MarkupEncodeOptions, MarkupFidelity, MarkupLoss};
use crate::markup::{Inline, MarkupBlock, MarkupDoc};

use super::{inline_plain_text, typst_id};

/// Narrow Typst emitter for semantic markup documents.
pub(super) struct TypstEncoder {
    preserve_raw: bool,
    fidelity: MarkupFidelity,
}

impl TypstEncoder {
    /// Creates a Typst encoder with the requested raw-fragment policy.
    pub(super) fn new(opts: &MarkupEncodeOptions) -> Self {
        Self {
            preserve_raw: opts.preserve_raw,
            fidelity: MarkupFidelity::exact(typst_id()),
        }
    }

    /// Renders the document as Typst source.
    pub(super) fn write_doc(&mut self, doc: &MarkupDoc) -> String {
        self.render_blocks(&doc.blocks)
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
            MarkupBlock::MathBlock { source, .. } => {
                out.push_str("$ ");
                out.push_str(source.text.trim());
                out.push_str(" $");
            }
            MarkupBlock::List { ordered, items, .. } => self.write_list(*ordered, items, out),
            MarkupBlock::Table { header, rows, .. } => self.write_table(header, rows, out),
            MarkupBlock::Figure { src, caption, .. } => self.write_figure(src, caption, out),
            MarkupBlock::Raw { backend, text, .. } if backend == &typst_id() => {
                self.write_raw(text, "raw-block", out);
            }
            MarkupBlock::Raw { text, .. } => self.write_raw(text, "raw-block", out),
            MarkupBlock::Quote { .. } => {
                self.drop_block("quote", "typst quote output is not supported")
            }
        }
    }

    fn write_code_block(&mut self, lang: &Option<String>, code: &str, out: &mut String) {
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

    fn write_list(&mut self, ordered: bool, items: &[Vec<MarkupBlock>], out: &mut String) {
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(if ordered { "+ " } else { "- " });
            out.push_str(&self.render_blocks(item).replace('\n', "\n  "));
        }
    }

    fn write_table(&mut self, header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
        let width = header
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1);
        out.push_str("#table(columns: ");
        out.push_str(&width.to_string());
        for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
            for cell in row {
                out.push_str(",\n  [");
                self.write_inlines(cell, out);
                out.push(']');
            }
        }
        out.push_str("\n)");
    }

    fn write_figure(&mut self, src: &str, caption: &[Inline], out: &mut String) {
        out.push_str("#figure(image(\"");
        write_string(src, out);
        out.push_str("\"), caption: [");
        self.write_inlines(caption, out);
        out.push_str("])");
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
                Inline::Link { label, target } if inline_plain_text(label) == *target => {
                    out.push_str(target);
                }
                Inline::Link { .. } => {
                    self.fidelity.dropped.push(MarkupLoss {
                        path: "link-label".to_owned(),
                        reason: "typst link labels are preserved only when label equals target"
                            .to_owned(),
                    });
                }
                Inline::Math(source) => {
                    out.push('$');
                    out.push_str(source.text.trim());
                    out.push('$');
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
                reason: "raw typst fragment omitted".to_owned(),
            });
        }
    }

    fn drop_block(&mut self, path: &str, reason: &str) {
        self.fidelity.dropped.push(MarkupLoss {
            path: path.to_owned(),
            reason: reason.to_owned(),
        });
    }
}

fn write_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        if matches!(ch, '#' | '*' | '_' | '`' | '$' | '@' | '<' | '>' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
}

fn write_string(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
}
