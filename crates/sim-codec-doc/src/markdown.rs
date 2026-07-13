//! Markdown backend implementation over `pulldown-cmark`.

use std::collections::BTreeMap;
use std::mem;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::backend::{
    MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions, MarkupError, MarkupFidelity,
    MarkupLoss,
};
use crate::markdown_writer::MarkdownEncoder;
use crate::markup::{BackendId, Inline, MarkupBlock, MarkupDoc, MathSource, SourceDoc, Span};

type MarkdownEvent = (Event<'static>, Range<usize>);

/// CommonMark/GFM-compatible Markdown backend.
#[derive(Clone, Debug, Default)]
pub struct MarkdownBackend;

impl MarkupBackend for MarkdownBackend {
    fn id(&self) -> BackendId {
        markdown_id()
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        let events = Parser::new_ext(input, markdown_options())
            .into_offset_iter()
            .map(|(event, range)| (event.into_static(), range))
            .collect();
        let mut parser = MarkdownParser::new(input, events, opts);
        let blocks = parser.parse_blocks_until(|_| false);
        let title = blocks.iter().find_map(|block| match block {
            MarkupBlock::Heading { level: 1, text, .. } => Some(inline_plain_text(text)),
            _ => None,
        });
        let source = opts.preserve_source.then(|| SourceDoc {
            backend: markdown_id(),
            text: input.to_owned(),
        });
        Ok((
            MarkupDoc {
                title,
                blocks,
                attrs: BTreeMap::new(),
                source,
            },
            parser.fidelity,
        ))
    }

    fn encode(
        &self,
        doc: &MarkupDoc,
        opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        let mut encoder = MarkdownEncoder::new(opts);
        let source = encoder.write_doc(doc);
        if opts.fail_on_loss && !encoder.fidelity.dropped.is_empty() {
            return Err(MarkupError::Encode(format!(
                "markdown encode dropped {} raw fragment(s)",
                encoder.fidelity.dropped.len()
            )));
        }
        Ok((source, encoder.fidelity))
    }
}

struct MarkdownParser<'a> {
    input: &'a str,
    events: Vec<MarkdownEvent>,
    index: usize,
    preserve_raw: bool,
    fidelity: MarkupFidelity,
}

impl<'a> MarkdownParser<'a> {
    fn new(input: &'a str, events: Vec<MarkdownEvent>, opts: &MarkupDecodeOptions) -> Self {
        Self {
            input,
            events,
            index: 0,
            preserve_raw: opts.preserve_raw,
            fidelity: MarkupFidelity::exact(markdown_id()),
        }
    }

    fn parse_blocks_until<F>(&mut self, stop: F) -> Vec<MarkupBlock>
    where
        F: Fn(&TagEnd) -> bool + Copy,
    {
        let mut blocks = Vec::new();
        let mut loose = Vec::new();
        while let Some((event, range)) = self.next() {
            match event {
                Event::End(end) if stop(&end) => {
                    self.index -= 1;
                    break;
                }
                Event::End(_) => self.flush_loose(&mut blocks, &mut loose),
                Event::Start(tag) => {
                    self.flush_loose(&mut blocks, &mut loose);
                    self.push_block(tag, range, &mut blocks);
                }
                Event::DisplayMath(text) => {
                    self.flush_loose(&mut blocks, &mut loose);
                    blocks.push(MarkupBlock::MathBlock {
                        source: tex_math(text),
                        span: Some(span(range.start, range.end)),
                    });
                }
                Event::Rule => {
                    self.flush_loose(&mut blocks, &mut loose);
                    if let Some(raw) = self.raw_block(self.slice(&range), "rule", &range) {
                        blocks.push(raw);
                    }
                }
                other => self.push_inline_event(other, range, &mut loose),
            }
        }
        self.flush_loose(&mut blocks, &mut loose);
        blocks
    }

    fn push_block(
        &mut self,
        tag: Tag<'static>,
        range: Range<usize>,
        blocks: &mut Vec<MarkupBlock>,
    ) {
        match tag {
            Tag::Paragraph => blocks.push(self.parse_paragraph(range, Vec::new())),
            Tag::Heading { level, id, .. } => blocks.push(self.parse_heading(level, id, range)),
            Tag::CodeBlock(kind) => blocks.push(self.parse_code_block(kind, range)),
            Tag::BlockQuote(_) => blocks.push(self.parse_quote(range)),
            Tag::List(start) => blocks.push(self.parse_list(start.is_some(), range)),
            Tag::Table(_) => blocks.push(self.parse_table(range)),
            Tag::HtmlBlock => {
                if let Some(raw) = self.raw_container(range, TagEnd::HtmlBlock, "html-block") {
                    blocks.push(raw);
                }
            }
            Tag::FootnoteDefinition(_) => {
                if let Some(raw) =
                    self.raw_container(range, TagEnd::FootnoteDefinition, "footnote-definition")
                {
                    blocks.push(raw);
                }
            }
            other => {
                let end = other.to_end();
                if let Some(raw) = self.raw_container(range, end, "unsupported-block") {
                    blocks.push(raw);
                }
            }
        }
    }

    fn parse_paragraph(&mut self, start: Range<usize>, mut prefix: Vec<Inline>) -> MarkupBlock {
        let (mut content, end) = self.collect_inlines_until(|end| *end == TagEnd::Paragraph);
        prefix.append(&mut content);
        if prefix.len() == 1
            && matches!(prefix[0], Inline::Math(_))
            && self
                .input
                .get(start.start..end.end)
                .map(str::trim)
                .is_some_and(|source| source.starts_with("$$") && source.ends_with("$$"))
        {
            let Inline::Math(source) = prefix.remove(0) else {
                unreachable!();
            };
            return MarkupBlock::MathBlock {
                source,
                span: Some(span(start.start, end.end)),
            };
        }
        MarkupBlock::Paragraph {
            content: prefix,
            span: Some(span(start.start, end.end)),
        }
    }

    fn parse_heading(
        &mut self,
        level: HeadingLevel,
        id: Option<pulldown_cmark::CowStr<'static>>,
        start: Range<usize>,
    ) -> MarkupBlock {
        let (text, end) = self.collect_inlines_until(|end| matches!(end, TagEnd::Heading(_)));
        MarkupBlock::Heading {
            level: heading_level(level),
            text,
            id: id.map(|value| value.to_string()),
            span: Some(span(start.start, end.end)),
        }
    }

    fn parse_code_block(
        &mut self,
        kind: CodeBlockKind<'static>,
        start: Range<usize>,
    ) -> MarkupBlock {
        let mut code = String::new();
        let mut end = start.clone();
        while let Some((event, range)) = self.next() {
            end = range.clone();
            match event {
                Event::End(TagEnd::CodeBlock) => break,
                Event::Text(text) => code.push_str(&text),
                Event::Code(text) => code.push_str(&text),
                Event::SoftBreak | Event::HardBreak => code.push('\n'),
                _ => {}
            }
        }
        let lang = match kind {
            CodeBlockKind::Fenced(info) => info.split_whitespace().next().map(str::to_owned),
            CodeBlockKind::Indented => None,
        };
        if matches!(lang.as_deref(), Some("math" | "tex")) {
            MarkupBlock::MathBlock {
                source: MathSource {
                    notation: "tex".to_owned(),
                    text: code,
                },
                span: Some(span(start.start, end.end)),
            }
        } else {
            MarkupBlock::CodeBlock {
                lang,
                code,
                span: Some(span(start.start, end.end)),
            }
        }
    }

    fn parse_quote(&mut self, start: Range<usize>) -> MarkupBlock {
        let blocks = self.parse_blocks_until(|end| matches!(end, TagEnd::BlockQuote(_)));
        let end = self.consume_end(|end| matches!(end, TagEnd::BlockQuote(_)), &start);
        MarkupBlock::Quote {
            blocks,
            span: Some(span(start.start, end.end)),
        }
    }

    fn parse_list(&mut self, ordered: bool, start: Range<usize>) -> MarkupBlock {
        let mut items = Vec::new();
        let mut end = start.clone();
        while let Some((event, range)) = self.next() {
            end = range.clone();
            match event {
                Event::End(TagEnd::List(_)) => break,
                Event::Start(Tag::Item) => {
                    let item = self.parse_blocks_until(|end| *end == TagEnd::Item);
                    end = self.consume_end(|end| *end == TagEnd::Item, &range);
                    items.push(item);
                }
                _ => {}
            }
        }
        MarkupBlock::List {
            ordered,
            items,
            span: Some(span(start.start, end.end)),
        }
    }

    fn parse_table(&mut self, start: Range<usize>) -> MarkupBlock {
        let mut header = Vec::new();
        let mut rows = Vec::new();
        let mut current_row: Option<Vec<Vec<Inline>>> = None;
        let mut in_head = false;
        let mut end = start.clone();
        while let Some((event, range)) = self.next() {
            end = range.clone();
            match event {
                Event::End(TagEnd::Table) => break,
                Event::Start(Tag::TableHead) => {
                    in_head = true;
                    current_row = Some(Vec::new());
                }
                Event::End(TagEnd::TableHead) => {
                    if let Some(row) = current_row.take() {
                        header = row;
                    }
                    in_head = false;
                }
                Event::Start(Tag::TableRow) => current_row = Some(Vec::new()),
                Event::End(TagEnd::TableRow) => {
                    if let Some(row) = current_row.take() {
                        if in_head {
                            header = row;
                        } else {
                            rows.push(row);
                        }
                    }
                }
                Event::Start(Tag::TableCell) => {
                    let (cell, cell_end) =
                        self.collect_inlines_until(|end| *end == TagEnd::TableCell);
                    end = cell_end;
                    current_row.get_or_insert_with(Vec::new).push(cell);
                }
                _ => {}
            }
        }
        MarkupBlock::Table {
            header,
            rows,
            span: Some(span(start.start, end.end)),
        }
    }

    fn collect_inlines_until<F>(&mut self, stop: F) -> (Vec<Inline>, Range<usize>)
    where
        F: Fn(&TagEnd) -> bool + Copy,
    {
        let mut items = Vec::new();
        let mut end = self.current_end();
        while let Some((event, range)) = self.next() {
            end = range.clone();
            match event {
                Event::End(tag_end) if stop(&tag_end) => break,
                other => self.push_inline_event(other, range, &mut items),
            }
        }
        (items, end)
    }

    fn push_inline_event(
        &mut self,
        event: Event<'static>,
        range: Range<usize>,
        items: &mut Vec<Inline>,
    ) {
        match event {
            Event::Text(text) => items.push(Inline::Text(text.to_string())),
            Event::Code(text) => items.push(Inline::Code(text.to_string())),
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                items.push(Inline::Math(tex_math(text)));
            }
            Event::SoftBreak | Event::HardBreak => items.push(Inline::Text("\n".to_owned())),
            Event::Html(text) | Event::InlineHtml(text) => {
                if let Some(raw) = self.raw_inline(text.to_string(), "html") {
                    items.push(raw);
                }
            }
            Event::FootnoteReference(label) => {
                if let Some(raw) = self.raw_inline(format!("[^{label}]"), "footnote-reference") {
                    items.push(raw);
                }
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                if let Some(raw) = self.raw_inline(marker.to_owned(), "task-list-marker") {
                    items.push(raw);
                }
            }
            Event::Rule => {
                if let Some(raw) = self.raw_inline(self.slice(&range), "rule") {
                    items.push(raw);
                }
            }
            Event::Start(Tag::Emphasis) => {
                let (children, _) = self.collect_inlines_until(|end| *end == TagEnd::Emphasis);
                items.push(Inline::Emph(children));
            }
            Event::Start(Tag::Strong) => {
                let (children, _) = self.collect_inlines_until(|end| *end == TagEnd::Strong);
                items.push(Inline::Strong(children));
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let (label, _) = self.collect_inlines_until(|end| *end == TagEnd::Link);
                items.push(Inline::Link {
                    label,
                    target: dest_url.to_string(),
                });
            }
            Event::Start(tag) => {
                let end = tag.to_end();
                if let Some(raw) = self.raw_inline_container(range, end, "unsupported-inline") {
                    items.push(raw);
                }
            }
            Event::End(_) => {}
        }
    }

    fn raw_container(
        &mut self,
        start: Range<usize>,
        target: TagEnd,
        path: &str,
    ) -> Option<MarkupBlock> {
        let (raw, end) = self.consume_raw_container(start.clone(), target);
        self.raw_block(raw, path, &(start.start..end))
    }

    fn raw_inline_container(
        &mut self,
        start: Range<usize>,
        target: TagEnd,
        path: &str,
    ) -> Option<Inline> {
        let (raw, _) = self.consume_raw_container(start, target);
        self.raw_inline(raw, path)
    }

    fn consume_raw_container(&mut self, start: Range<usize>, target: TagEnd) -> (String, usize) {
        let mut depth = 1usize;
        let mut end = start.end;
        while let Some((event, range)) = self.next() {
            end = range.end;
            match event {
                Event::Start(tag) if tag.to_end() == target => depth += 1,
                Event::End(tag_end) if tag_end == target => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        (
            self.input.get(start.start..end).unwrap_or("").to_owned(),
            end,
        )
    }

    fn raw_block(&mut self, raw: String, path: &str, range: &Range<usize>) -> Option<MarkupBlock> {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(raw.clone());
            Some(MarkupBlock::Raw {
                backend: markdown_id(),
                text: raw,
                span: Some(span(range.start, range.end)),
            })
        } else {
            self.drop_raw(path, "unsupported markdown block");
            None
        }
    }

    fn raw_inline(&mut self, raw: String, path: &str) -> Option<Inline> {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(raw.clone());
            Some(Inline::Raw {
                backend: markdown_id(),
                text: raw,
            })
        } else {
            self.drop_raw(path, "unsupported markdown inline");
            None
        }
    }

    fn drop_raw(&mut self, path: &str, reason: &str) {
        self.fidelity.dropped.push(MarkupLoss {
            path: path.to_owned(),
            reason: reason.to_owned(),
        });
    }

    fn flush_loose(&mut self, blocks: &mut Vec<MarkupBlock>, loose: &mut Vec<Inline>) {
        if !loose.is_empty() {
            blocks.push(MarkupBlock::Paragraph {
                content: mem::take(loose),
                span: None,
            });
        }
    }

    fn consume_end<F>(&mut self, stop: F, fallback: &Range<usize>) -> Range<usize>
    where
        F: Fn(&TagEnd) -> bool,
    {
        match self.next() {
            Some((Event::End(end), range)) if stop(&end) => range,
            Some(_) => fallback.clone(),
            None => fallback.clone(),
        }
    }

    fn next(&mut self) -> Option<MarkdownEvent> {
        let event = self.events.get(self.index).cloned();
        if event.is_some() {
            self.index += 1;
        }
        event
    }

    fn current_end(&self) -> Range<usize> {
        self.events
            .get(self.index.saturating_sub(1))
            .map(|(_, range)| range.clone())
            .unwrap_or(0..0)
    }

    fn slice(&self, range: &Range<usize>) -> String {
        self.input.get(range.clone()).unwrap_or("").to_owned()
    }
}

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_MATH);
    options
}

fn markdown_id() -> BackendId {
    BackendId::new("markdown")
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn tex_math(text: pulldown_cmark::CowStr<'static>) -> MathSource {
    MathSource {
        notation: "tex".to_owned(),
        text: text.trim_matches('\n').to_owned(),
    }
}

fn span(start: usize, end: usize) -> Span {
    Span { start, end }
}

fn inline_plain_text(items: &[Inline]) -> String {
    let mut text = String::new();
    for item in items {
        match item {
            Inline::Text(value) | Inline::Code(value) => text.push_str(value),
            Inline::Emph(children) | Inline::Strong(children) => {
                text.push_str(&inline_plain_text(children));
            }
            Inline::Link { label, .. } => text.push_str(&inline_plain_text(label)),
            Inline::Math(source) => text.push_str(&source.text),
            Inline::Raw { text: raw, .. } => text.push_str(raw),
        }
    }
    text
}
