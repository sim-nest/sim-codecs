//! Safe LaTeX backend implementation over `codebook-tree-sitter-latex`.

use std::collections::BTreeMap;

use tree_sitter::Parser;

use crate::backend::{
    MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions, MarkupError, MarkupFidelity,
    MarkupLoss,
};
use crate::markup::{BackendId, MarkupBlock, MarkupDoc, SourceDoc, Span};

#[path = "latex_support.rs"]
mod latex_support;

#[path = "latex_writer.rs"]
mod latex_writer;

use latex_support::{
    after_line, begin_environment_at, command_arg_anywhere, command_arg_at, command_arg_end,
    find_unescaped, inline_plain_text, line_bounds, offset_block_span, paragraph_line,
    parse_inlines, span, split_items, starts_command, table_rows, tex_math, trim_edge_newlines,
    unescape_text,
};
use latex_writer::LatexEncoder;

/// Safe LaTeX article-subset backend.
#[derive(Clone, Debug, Default)]
pub struct LatexBackend;

impl MarkupBackend for LatexBackend {
    fn id(&self) -> BackendId {
        latex_id()
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        let mut decoder = LatexDecoder::new(input, opts);
        decoder.check_syntax()?;
        let blocks = decoder.parse_blocks();
        let title = decoder.title.clone().or_else(|| {
            blocks.iter().find_map(|block| match block {
                MarkupBlock::Heading { level: 1, text, .. } => Some(inline_plain_text(text)),
                _ => None,
            })
        });
        let source = opts.preserve_source.then(|| SourceDoc {
            backend: latex_id(),
            text: input.to_owned(),
        });
        Ok((
            MarkupDoc {
                title,
                blocks,
                attrs: BTreeMap::new(),
                source,
            },
            decoder.fidelity,
        ))
    }

    fn encode(
        &self,
        doc: &MarkupDoc,
        opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        let mut encoder = LatexEncoder::new(opts);
        let source = encoder.write_doc(doc);
        if opts.fail_on_loss && !encoder.fidelity().dropped.is_empty() {
            return Err(MarkupError::Encode(format!(
                "latex encode dropped {} unsupported fragment(s)",
                encoder.fidelity().dropped.len()
            )));
        }
        Ok((source, encoder.into_fidelity()))
    }
}

struct LatexDecoder<'a> {
    input: &'a str,
    preserve_raw: bool,
    title: Option<String>,
    fidelity: MarkupFidelity,
}

impl<'a> LatexDecoder<'a> {
    fn new(input: &'a str, opts: &MarkupDecodeOptions) -> Self {
        Self {
            input,
            preserve_raw: opts.preserve_raw,
            title: None,
            fidelity: MarkupFidelity::exact(latex_id()),
        }
    }

    fn check_syntax(&mut self) -> Result<(), MarkupError> {
        let mut parser = Parser::new();
        let language = codebook_tree_sitter_latex::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|err| MarkupError::Decode(format!("cannot load latex grammar: {err}")))?;
        let tree = parser
            .parse(self.input, None)
            .ok_or_else(|| MarkupError::Decode("tree-sitter latex parse failed".to_owned()))?;
        if tree.root_node().has_error() {
            self.fidelity.warnings.push(
                "latex syntax contains tree-sitter error nodes; unsupported source is preserved raw"
                    .to_owned(),
            );
        }
        Ok(())
    }

    fn parse_blocks(&mut self) -> Vec<MarkupBlock> {
        let mut blocks = Vec::new();
        let mut paragraph = PendingParagraph::default();
        let mut cursor = 0;
        while cursor < self.input.len() {
            let (line_end, next_line) = line_bounds(self.input, cursor);
            let line = &self.input[cursor..line_end];
            let Some(view) = LineView::new(cursor, line) else {
                self.flush_paragraph(&mut blocks, &mut paragraph);
                cursor = next_line;
                continue;
            };
            if view.trimmed.starts_with('%') {
                cursor = next_line;
                continue;
            }
            if self.handle_title(view.start, &mut blocks, &mut paragraph) {
                cursor = after_line(self.input, command_arg_end(self.input, view.start));
                continue;
            }
            if self.skip_document_wrapper(view.trimmed) {
                self.flush_paragraph(&mut blocks, &mut paragraph);
                cursor = next_line;
                continue;
            }
            if let Some((block, next)) = self.heading(view.start) {
                self.flush_paragraph(&mut blocks, &mut paragraph);
                blocks.push(block);
                cursor = after_line(self.input, next);
                continue;
            }
            if let Some((block, next)) = self.display_math(view.start) {
                self.flush_paragraph(&mut blocks, &mut paragraph);
                blocks.push(block);
                cursor = next;
                continue;
            }
            if let Some((blocks_from_env, next)) = self.environment(view.start) {
                self.flush_paragraph(&mut blocks, &mut paragraph);
                blocks.extend(blocks_from_env);
                cursor = next;
                continue;
            }
            if let Some((raw, next)) = self.raw_command(view.start, line_end) {
                self.flush_paragraph(&mut blocks, &mut paragraph);
                if let Some(block) = raw {
                    blocks.push(block);
                }
                cursor = next;
                continue;
            }
            if let Some((text, end)) = paragraph_line(self.input, view.start, view.end) {
                paragraph.push(text, view.start, end);
            }
            cursor = next_line;
        }
        self.flush_paragraph(&mut blocks, &mut paragraph);
        blocks
    }

    fn handle_title(
        &mut self,
        start: usize,
        blocks: &mut Vec<MarkupBlock>,
        paragraph: &mut PendingParagraph,
    ) -> bool {
        if let Some(arg) = command_arg_at(self.input, start, "\\title") {
            self.flush_paragraph(blocks, paragraph);
            let inlines =
                parse_inlines(arg.text(self.input), self.preserve_raw, &mut self.fidelity);
            self.title = Some(inline_plain_text(&inlines));
            return true;
        }
        false
    }

    fn skip_document_wrapper(&self, trimmed: &str) -> bool {
        trimmed.starts_with("\\begin{document}")
            || trimmed.starts_with("\\end{document}")
            || trimmed.starts_with("\\maketitle")
    }

    fn heading(&mut self, start: usize) -> Option<(MarkupBlock, usize)> {
        let (command, level) = [
            ("\\subsubsection", 3),
            ("\\subsection", 2),
            ("\\section", 1),
        ]
        .into_iter()
        .find(|(command, _)| starts_command(self.input, start, command))?;
        let arg = command_arg_at(self.input, start, command)?;
        Some((
            MarkupBlock::Heading {
                level,
                text: parse_inlines(arg.text(self.input), self.preserve_raw, &mut self.fidelity),
                id: None,
                span: Some(span(start, arg.next)),
            },
            arg.next,
        ))
    }

    fn display_math(&mut self, start: usize) -> Option<(MarkupBlock, usize)> {
        let (content_start, close, after) = if self.input[start..].starts_with("\\[") {
            let content_start = start + 2;
            let close = find_unescaped(self.input, content_start, "\\]")?;
            (content_start, close, close + 2)
        } else if self.input[start..].starts_with("$$") {
            let content_start = start + 2;
            let close = find_unescaped(self.input, content_start, "$$")?;
            (content_start, close, close + 2)
        } else {
            return None;
        };
        Some((
            MarkupBlock::MathBlock {
                source: tex_math(&self.input[content_start..close]),
                span: Some(span(start, after)),
            },
            after,
        ))
    }

    fn environment(&mut self, start: usize) -> Option<(Vec<MarkupBlock>, usize)> {
        let env = begin_environment_at(self.input, start)?;
        let blocks = match env.name {
            "itemize" => vec![self.list(false, &env)],
            "enumerate" => vec![self.list(true, &env)],
            "verbatim" => vec![MarkupBlock::CodeBlock {
                lang: None,
                code: trim_edge_newlines(env.content(self.input)).to_owned(),
                span: Some(span(start, env.after_end)),
            }],
            "tabular" => vec![self.table(&env)],
            "figure" => self.figure(&env).into_iter().collect(),
            "equation" | "equation*" => vec![MarkupBlock::MathBlock {
                source: tex_math(env.content(self.input)),
                span: Some(span(start, env.after_end)),
            }],
            "quote" => vec![MarkupBlock::Quote {
                blocks: self.blocks_from_fragment(env.content(self.input), env.content_start),
                span: Some(span(start, env.after_end)),
            }],
            _ => self
                .raw_block(
                    self.input[start..env.after_end].to_owned(),
                    "environment",
                    "unsupported latex environment is preserved but not executed",
                    Some(span(start, env.after_end)),
                )
                .into_iter()
                .collect(),
        };
        Some((blocks, env.after_end))
    }

    fn raw_command(
        &mut self,
        start: usize,
        line_end: usize,
    ) -> Option<(Option<MarkupBlock>, usize)> {
        let (path, reason) = if starts_command(self.input, start, "\\input") {
            ("input", "latex input is preserved but not resolved")
        } else if starts_command(self.input, start, "\\include") {
            ("include", "latex include is preserved but not resolved")
        } else if starts_command(self.input, start, "\\bibliography")
            || starts_command(self.input, start, "\\addbibresource")
        {
            (
                "bibliography",
                "latex bibliography command is preserved but not read",
            )
        } else if starts_command(self.input, start, "\\documentclass")
            || starts_command(self.input, start, "\\usepackage")
        {
            (
                "preamble",
                "latex preamble command is preserved as raw source",
            )
        } else if self.input[start..].starts_with('\\') {
            (
                "command",
                "unsupported latex command is preserved but not executed",
            )
        } else {
            return None;
        };
        let end = command_arg_end(self.input, start).max(line_end);
        let text = self.input[start..end].trim_end().to_owned();
        let raw = self.raw_block(text, path, reason, Some(span(start, end)));
        Some((raw, after_line(self.input, end)))
    }

    fn list(&mut self, ordered: bool, env: &Environment<'_>) -> MarkupBlock {
        let items = split_items(env.content(self.input))
            .into_iter()
            .map(|item| {
                let content = item.trim();
                if content.is_empty() {
                    Vec::new()
                } else {
                    vec![MarkupBlock::Paragraph {
                        content: parse_inlines(content, self.preserve_raw, &mut self.fidelity),
                        span: None,
                    }]
                }
            })
            .collect();
        MarkupBlock::List {
            ordered,
            items,
            span: Some(span(env.start, env.after_end)),
        }
    }

    fn table(&mut self, env: &Environment<'_>) -> MarkupBlock {
        let rows = table_rows(env.content(self.input))
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| parse_inlines(cell.trim(), self.preserve_raw, &mut self.fidelity))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut iter = rows.into_iter();
        let header = iter.next().unwrap_or_default();
        MarkupBlock::Table {
            header,
            rows: iter.collect(),
            span: Some(span(env.start, env.after_end)),
        }
    }

    fn figure(&mut self, env: &Environment<'_>) -> Option<MarkupBlock> {
        let content = env.content(self.input);
        let src = command_arg_anywhere(content, "\\includegraphics")
            .map(|arg| unescape_text(arg.text(content)))
            .unwrap_or_default();
        let caption = command_arg_anywhere(content, "\\caption")
            .map(|arg| parse_inlines(arg.text(content), self.preserve_raw, &mut self.fidelity))
            .unwrap_or_default();
        if src.is_empty() && caption.is_empty() {
            return self.raw_block(
                self.input[env.start..env.after_end].to_owned(),
                "figure",
                "unsupported latex figure is preserved as raw source",
                Some(span(env.start, env.after_end)),
            );
        }
        Some(MarkupBlock::Figure {
            src,
            caption,
            span: Some(span(env.start, env.after_end)),
        })
    }

    fn blocks_from_fragment(&mut self, source: &str, offset: usize) -> Vec<MarkupBlock> {
        let mut decoder = LatexDecoder {
            input: source,
            preserve_raw: self.preserve_raw,
            title: None,
            fidelity: MarkupFidelity::exact(latex_id()),
        };
        let mut blocks = decoder.parse_blocks();
        self.fidelity
            .preserved_raw
            .extend(decoder.fidelity.preserved_raw);
        self.fidelity.dropped.extend(decoder.fidelity.dropped);
        self.fidelity.warnings.extend(decoder.fidelity.warnings);
        for block in &mut blocks {
            offset_block_span(block, offset);
        }
        blocks
    }

    fn flush_paragraph(&mut self, blocks: &mut Vec<MarkupBlock>, paragraph: &mut PendingParagraph) {
        if let Some((text, start, end)) = paragraph.take() {
            blocks.push(MarkupBlock::Paragraph {
                content: parse_inlines(&text, self.preserve_raw, &mut self.fidelity),
                span: Some(span(start, end)),
            });
        }
    }

    fn raw_block(
        &mut self,
        text: String,
        path: &str,
        reason: &str,
        span: Option<Span>,
    ) -> Option<MarkupBlock> {
        self.fidelity.warnings.push(reason.to_owned());
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(text.clone());
            Some(MarkupBlock::Raw {
                backend: latex_id(),
                text,
                span,
            })
        } else {
            self.fidelity.dropped.push(MarkupLoss {
                path: path.to_owned(),
                reason: reason.to_owned(),
            });
            None
        }
    }
}

#[derive(Default)]
struct PendingParagraph {
    text: String,
    start: Option<usize>,
    end: usize,
}

impl PendingParagraph {
    fn push(&mut self, text: &str, start: usize, end: usize) {
        if text.trim().is_empty() {
            return;
        }
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.text.push_str(text.trim());
        self.start.get_or_insert(start);
        self.end = end;
    }

    fn take(&mut self) -> Option<(String, usize, usize)> {
        if self.text.trim().is_empty() {
            self.text.clear();
            self.start = None;
            return None;
        }
        Some((std::mem::take(&mut self.text), self.start.take()?, self.end))
    }
}

#[derive(Clone, Copy)]
struct LineView<'a> {
    trimmed: &'a str,
    start: usize,
    end: usize,
}

impl<'a> LineView<'a> {
    fn new(line_start: usize, line: &'a str) -> Option<Self> {
        let leading = line.len() - line.trim_start().len();
        let trailing = line.len() - line.trim_end().len();
        let start = line_start + leading;
        let end = line_start + line.len() - trailing;
        (start < end).then_some(Self {
            trimmed: &line[leading..line.len() - trailing],
            start,
            end,
        })
    }
}

#[derive(Clone, Copy)]
struct BracedArg {
    content_start: usize,
    content_end: usize,
    next: usize,
}

impl BracedArg {
    fn text<'a>(&self, input: &'a str) -> &'a str {
        &input[self.content_start..self.content_end]
    }
}

struct Environment<'a> {
    name: &'a str,
    start: usize,
    content_start: usize,
    content_end: usize,
    after_end: usize,
}

impl<'a> Environment<'a> {
    fn content<'b>(&self, input: &'b str) -> &'b str {
        &input[self.content_start..self.content_end]
    }
}

fn latex_id() -> BackendId {
    BackendId::new("latex")
}
