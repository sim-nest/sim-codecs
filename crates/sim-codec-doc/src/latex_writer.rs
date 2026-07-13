//! Deterministic LaTeX writer used by the safe LaTeX backend.

use crate::backend::{MarkupEncodeOptions, MarkupFidelity, MarkupLoss};
use crate::markup::{Inline, MarkupBlock, MarkupDoc};

use super::latex_id;

/// Narrow LaTeX article-subset emitter.
pub(super) struct LatexEncoder {
    preserve_raw: bool,
    fidelity: MarkupFidelity,
}

impl LatexEncoder {
    /// Creates a LaTeX encoder with the requested raw-fragment policy.
    pub(super) fn new(opts: &MarkupEncodeOptions) -> Self {
        Self {
            preserve_raw: opts.preserve_raw,
            fidelity: MarkupFidelity::exact(latex_id()),
        }
    }

    /// Renders the document as LaTeX source.
    pub(super) fn write_doc(&mut self, doc: &MarkupDoc) -> String {
        let mut out = String::new();
        if let Some(title) = &doc.title {
            out.push_str("\\title{");
            write_text(title, &mut out);
            out.push_str("}\n\n");
        }
        out.push_str(&self.render_blocks(&doc.blocks));
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
            MarkupBlock::Heading { level, text, .. } => self.write_heading(*level, text, out),
            MarkupBlock::Paragraph { content, .. } => self.write_inlines(content, out),
            MarkupBlock::CodeBlock { code, .. } => self.write_code_block(code, out),
            MarkupBlock::MathBlock { source, .. } => {
                out.push_str("\\[");
                out.push_str(&source.text);
                out.push_str("\\]");
            }
            MarkupBlock::Quote { blocks, .. } => self.write_wrapped_env("quote", blocks, out),
            MarkupBlock::List { ordered, items, .. } => self.write_list(*ordered, items, out),
            MarkupBlock::Table { header, rows, .. } => self.write_table(header, rows, out),
            MarkupBlock::Figure { src, caption, .. } => self.write_figure(src, caption, out),
            MarkupBlock::Raw { backend, text, .. } if backend == &latex_id() => {
                self.write_raw(text, "raw-block", out);
            }
            MarkupBlock::Raw { text, .. } => self.write_raw(text, "raw-block", out),
        }
    }

    fn write_heading(&mut self, level: u8, text: &[Inline], out: &mut String) {
        let command = match level {
            0 | 1 => "section",
            2 => "subsection",
            3 => "subsubsection",
            4 => "paragraph",
            _ => "subparagraph",
        };
        out.push('\\');
        out.push_str(command);
        out.push('{');
        self.write_inlines(text, out);
        out.push('}');
    }

    fn write_code_block(&mut self, code: &str, out: &mut String) {
        out.push_str("\\begin{verbatim}\n");
        out.push_str(code);
        if !code.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("\\end{verbatim}");
    }

    fn write_wrapped_env(&mut self, env: &str, blocks: &[MarkupBlock], out: &mut String) {
        out.push_str("\\begin{");
        out.push_str(env);
        out.push_str("}\n");
        out.push_str(&self.render_blocks(blocks));
        out.push_str("\n\\end{");
        out.push_str(env);
        out.push('}');
    }

    fn write_list(&mut self, ordered: bool, items: &[Vec<MarkupBlock>], out: &mut String) {
        let env = if ordered { "enumerate" } else { "itemize" };
        out.push_str("\\begin{");
        out.push_str(env);
        out.push_str("}\n");
        for item in items {
            out.push_str("\\item ");
            out.push_str(&self.render_blocks(item).replace('\n', "\n  "));
            out.push('\n');
        }
        out.push_str("\\end{");
        out.push_str(env);
        out.push('}');
    }

    fn write_table(&mut self, header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
        let width = header
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1);
        out.push_str("\\begin{tabular}{");
        out.push_str(&"l".repeat(width));
        out.push_str("}\n");
        self.write_table_row(header, out);
        for row in rows {
            out.push_str(" \\\\\n");
            self.write_table_row(row, out);
        }
        out.push_str("\n\\end{tabular}");
    }

    fn write_table_row(&mut self, row: &[Vec<Inline>], out: &mut String) {
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                out.push_str(" & ");
            }
            self.write_inlines(cell, out);
        }
    }

    fn write_figure(&mut self, src: &str, caption: &[Inline], out: &mut String) {
        out.push_str("\\begin{figure}\n");
        if !src.is_empty() {
            out.push_str("\\includegraphics{");
            write_text(src, out);
            out.push_str("}\n");
        }
        out.push_str("\\caption{");
        self.write_inlines(caption, out);
        out.push_str("}\n\\end{figure}");
    }

    fn write_inlines(&mut self, items: &[Inline], out: &mut String) {
        for item in items {
            match item {
                Inline::Text(value) => write_text(value, out),
                Inline::Emph(children) => {
                    out.push_str("\\emph{");
                    self.write_inlines(children, out);
                    out.push('}');
                }
                Inline::Strong(children) => {
                    out.push_str("\\textbf{");
                    self.write_inlines(children, out);
                    out.push('}');
                }
                Inline::Code(value) => {
                    out.push_str("\\texttt{");
                    write_text(value, out);
                    out.push('}');
                }
                Inline::Link { label, target } => {
                    out.push_str("\\href{");
                    write_text(target, out);
                    out.push_str("}{");
                    self.write_inlines(label, out);
                    out.push('}');
                }
                Inline::Math(source) => {
                    out.push('$');
                    out.push_str(&source.text);
                    out.push('$');
                }
                Inline::Raw { backend, text } if backend == &latex_id() => {
                    self.write_raw(text, "raw-inline", out);
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
                reason: "raw latex fragment omitted".to_owned(),
            });
        }
    }
}

fn write_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '%' | '&' | '$' | '_' | '#' => {
                out.push('\\');
                out.push(ch);
            }
            '^' => out.push_str("\\^{}"),
            '~' => out.push_str("\\~{}"),
            other => out.push(other),
        }
    }
}
