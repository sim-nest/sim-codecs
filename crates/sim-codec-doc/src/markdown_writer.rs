//! Deterministic Markdown writer for the shared markup IR.

use crate::backend::{MarkupEncodeOptions, MarkupFidelity, MarkupLoss};
use crate::markdown::LinkDialect;
use crate::markup::{BackendId, Inline, MarkupBlock, MarkupDoc};

pub(crate) struct MarkdownEncoder {
    preserve_raw: bool,
    links: LinkDialect,
    pub(crate) fidelity: MarkupFidelity,
}

impl MarkdownEncoder {
    pub(crate) fn new(opts: &MarkupEncodeOptions, links: LinkDialect) -> Self {
        Self {
            preserve_raw: opts.preserve_raw,
            links,
            fidelity: MarkupFidelity::exact(BackendId::new("markdown")),
        }
    }

    pub(crate) fn write_doc(&mut self, doc: &MarkupDoc) -> String {
        self.render_blocks(&doc.blocks)
    }

    fn write_block(&mut self, block: &MarkupBlock, out: &mut String) {
        match block {
            MarkupBlock::Heading { level, text, .. } => {
                out.push_str(&"#".repeat(usize::from(*level).max(1)));
                out.push(' ');
                self.write_inlines(text, out);
            }
            MarkupBlock::Paragraph { content, .. } => self.write_inlines(content, out),
            MarkupBlock::CodeBlock { lang, code, .. } => self.write_code_block(lang, code, out),
            MarkupBlock::MathBlock { source, .. } => {
                out.push_str("$$\n");
                out.push_str(source.text.trim_matches('\n'));
                out.push_str("\n$$");
            }
            MarkupBlock::Quote { blocks, .. } => self.write_quote(blocks, out),
            MarkupBlock::List { ordered, items, .. } => self.write_list(*ordered, items, out),
            MarkupBlock::Table { header, rows, .. } => self.write_table(header, rows, out),
            MarkupBlock::Figure { src, caption, .. } => {
                out.push_str("![");
                self.write_inlines(caption, out);
                out.push_str("](");
                out.push_str(src);
                out.push(')');
            }
            MarkupBlock::Raw { text, .. } => self.write_raw(text, "raw-block", out),
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

    fn write_quote(&mut self, blocks: &[MarkupBlock], out: &mut String) {
        let text = self.render_blocks(blocks);
        for (index, line) in text.lines().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str("> ");
            out.push_str(line);
        }
    }

    fn write_list(&mut self, ordered: bool, items: &[Vec<MarkupBlock>], out: &mut String) {
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            if ordered {
                out.push_str(&format!("{}. ", index + 1));
            } else {
                out.push_str("- ");
            }
            out.push_str(&self.render_blocks(item).replace('\n', "\n  "));
        }
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

    fn write_table(&mut self, header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
        self.write_table_row(header, out);
        out.push('\n');
        out.push('|');
        for _ in 0..header.len() {
            out.push_str(" --- |");
        }
        for row in rows {
            out.push('\n');
            self.write_table_row(row, out);
        }
    }

    fn write_table_row(&mut self, row: &[Vec<Inline>], out: &mut String) {
        out.push('|');
        for cell in row {
            out.push(' ');
            self.write_inlines(cell, out);
            out.push_str(" |");
        }
    }

    fn write_inlines(&mut self, items: &[Inline], out: &mut String) {
        for item in items {
            match item {
                Inline::Text(value) => out.push_str(value),
                Inline::Emph(children) => {
                    out.push('*');
                    self.write_inlines(children, out);
                    out.push('*');
                }
                Inline::Strong(children) => {
                    out.push_str("**");
                    self.write_inlines(children, out);
                    out.push_str("**");
                }
                Inline::Code(value) => {
                    out.push('`');
                    out.push_str(value);
                    out.push('`');
                }
                Inline::Link { label, target } => match self.links {
                    LinkDialect::CommonMark => {
                        out.push('[');
                        self.write_inlines(label, out);
                        out.push_str("](");
                        out.push_str(target);
                        out.push(')');
                    }
                    LinkDialect::WikiLink => {
                        out.push_str("[[");
                        out.push_str(&escape_wikilink(target));
                        let label_text = inline_plain_text(label);
                        if label_text != *target {
                            out.push('|');
                            out.push_str(&escape_wikilink(&label_text));
                        }
                        out.push_str("]]");
                    }
                },
                Inline::Math(source) => {
                    out.push('$');
                    out.push_str(&source.text);
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
                reason: "raw markdown fragment omitted".to_owned(),
            });
        }
    }
}

fn inline_plain_text(items: &[Inline]) -> String {
    let mut text = String::new();
    for item in items {
        match item {
            Inline::Text(value) | Inline::Code(value) => text.push_str(value),
            Inline::Emph(children) | Inline::Strong(children) => {
                text.push_str(&inline_plain_text(children))
            }
            Inline::Link { label, .. } => text.push_str(&inline_plain_text(label)),
            Inline::Math(source) => text.push_str(&source.text),
            Inline::Raw { text: raw, .. } => text.push_str(raw),
        }
    }
    text
}

fn escape_wikilink(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\\', "%5C")
        .replace('|', "%7C")
        .replace(']', "%5D")
}
