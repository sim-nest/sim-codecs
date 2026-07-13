//! AsciiDoc backend implementation over `asciidork-parser`.

use std::collections::BTreeMap;

use asciidork_ast as adoc;
use asciidork_parser::prelude::{Bump, Parser, SourceFile};

use crate::backend::{
    MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions, MarkupError, MarkupFidelity,
    MarkupLoss,
};
use crate::markup::{BackendId, Inline, MarkupBlock, MarkupDoc, MathSource, SourceDoc, Span};

#[path = "asciidoc_support.rs"]
mod asciidoc_support;
#[path = "asciidoc_writer.rs"]
mod asciidoc_writer;

use asciidoc_support::{
    RawDirective, asciidoc_id, block_plain_text, block_start, inline_plain_text, link_target,
    mask_directives, raw_directives, span_from_loc, span_from_multi, symbol_text,
};
use asciidoc_writer::AsciiDocEncoder;

/// Safe AsciiDoc markup backend.
#[derive(Clone, Debug, Default)]
pub struct AsciiDocBackend;

impl MarkupBackend for AsciiDocBackend {
    fn id(&self) -> BackendId {
        asciidoc_id()
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        let directives = raw_directives(input);
        let masked = mask_directives(input, &directives);
        let parse_input = masked.as_ref();
        let bump = Bump::new();
        let parser = Parser::from_str(parse_input, SourceFile::Tmp, &bump);
        let result = parser.parse().map_err(|diagnostics| {
            MarkupError::Decode(
                diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;

        let mut decoder = AsciiDocDecoder::new(input, opts);
        decoder.fidelity.warnings.extend(
            result
                .warnings
                .into_iter()
                .map(|diagnostic| diagnostic.message),
        );
        decoder.record_directives(&directives);
        let title = result
            .document
            .title()
            .map(|title| inline_plain_text(&decoder.inlines(&title.main)));
        let parsed_blocks = decoder.blocks_from_content(&result.document.content);
        let blocks = decoder.merge_directives(parsed_blocks);
        let source = opts.preserve_source.then(|| SourceDoc {
            backend: asciidoc_id(),
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
        let mut encoder = AsciiDocEncoder::new(opts);
        let source = encoder.write_doc(doc);
        if opts.fail_on_loss && !encoder.fidelity().dropped.is_empty() {
            return Err(MarkupError::Encode(format!(
                "asciidoc encode dropped {} unsupported fragment(s)",
                encoder.fidelity().dropped.len()
            )));
        }
        Ok((source, encoder.into_fidelity()))
    }
}

struct AsciiDocDecoder<'a> {
    input: &'a str,
    preserve_raw: bool,
    fidelity: MarkupFidelity,
    raw_blocks: Vec<MarkupBlock>,
}

impl<'a> AsciiDocDecoder<'a> {
    fn new(input: &'a str, opts: &MarkupDecodeOptions) -> Self {
        Self {
            input,
            preserve_raw: opts.preserve_raw,
            fidelity: MarkupFidelity::exact(asciidoc_id()),
            raw_blocks: Vec::new(),
        }
    }

    fn blocks_from_content(&mut self, content: &adoc::DocContent<'_>) -> Vec<MarkupBlock> {
        match content {
            adoc::DocContent::Blocks(blocks) => self.blocks_from_blocks(blocks),
            adoc::DocContent::Sections(sectioned) => {
                let mut out = sectioned
                    .preamble
                    .as_ref()
                    .map(|blocks| self.blocks_from_blocks(blocks))
                    .unwrap_or_default();
                for section in &sectioned.sections {
                    out.extend(self.blocks_from_section(section));
                }
                out
            }
            adoc::DocContent::Parts(book) => {
                let mut out = book
                    .preamble
                    .as_ref()
                    .map(|blocks| self.blocks_from_blocks(blocks))
                    .unwrap_or_default();
                for section in &book.opening_special_sects {
                    out.extend(self.blocks_from_section(section));
                }
                for part in &book.parts {
                    out.push(MarkupBlock::Heading {
                        level: 1,
                        text: self.inlines(&part.title.text),
                        id: part.title.id.as_ref().map(ToString::to_string),
                        span: span_from_loc(part.title.meta.start_loc),
                    });
                    if let Some(intro) = &part.intro {
                        out.extend(self.blocks_from_blocks(intro));
                    }
                    for section in &part.sections {
                        out.extend(self.blocks_from_section(section));
                    }
                }
                for section in &book.closing_special_sects {
                    out.extend(self.blocks_from_section(section));
                }
                out
            }
        }
    }

    fn blocks_from_blocks(&mut self, blocks: &[adoc::Block<'_>]) -> Vec<MarkupBlock> {
        blocks
            .iter()
            .flat_map(|block| self.blocks_from_block(block))
            .collect()
    }

    fn blocks_from_section(&mut self, section: &adoc::Section<'_>) -> Vec<MarkupBlock> {
        let mut blocks = vec![MarkupBlock::Heading {
            level: section.level.saturating_add(1).clamp(1, 6),
            text: self.inlines(&section.heading),
            id: section.id.as_ref().map(ToString::to_string),
            span: span_from_multi(&section.loc),
        }];
        blocks.extend(self.blocks_from_blocks(&section.blocks));
        blocks
    }

    fn blocks_from_block(&mut self, block: &adoc::Block<'_>) -> Vec<MarkupBlock> {
        let span = span_from_multi(&block.loc);
        match &block.content {
            adoc::BlockContent::Compound(blocks)
                if block.context == adoc::BlockContext::BlockQuote =>
            {
                vec![MarkupBlock::Quote {
                    blocks: self.blocks_from_blocks(blocks),
                    span,
                }]
            }
            adoc::BlockContent::Compound(blocks) => self.blocks_from_blocks(blocks),
            adoc::BlockContent::Simple(inlines) => self.simple_block(block, inlines, span),
            adoc::BlockContent::Empty(empty) => self.empty_block(block, empty, span),
            adoc::BlockContent::Table(table) => {
                vec![MarkupBlock::Table {
                    header: table
                        .header_row
                        .as_ref()
                        .map(|row| self.row_cells(row))
                        .unwrap_or_default(),
                    rows: table.rows.iter().map(|row| self.row_cells(row)).collect(),
                    span,
                }]
            }
            adoc::BlockContent::Section(section) => self.blocks_from_section(section),
            adoc::BlockContent::DocumentAttribute(name, _) => self
                .raw_from_span(
                    span,
                    name,
                    "asciidoc document attribute is not semantic markup",
                )
                .into_iter()
                .collect(),
            adoc::BlockContent::QuotedParagraph { quote, .. } => vec![MarkupBlock::Quote {
                blocks: vec![MarkupBlock::Paragraph {
                    content: self.inlines(quote),
                    span: None,
                }],
                span,
            }],
            adoc::BlockContent::List { variant, items, .. } => {
                self.list_block(*variant, items, span)
            }
        }
    }

    fn simple_block(
        &mut self,
        block: &adoc::Block<'_>,
        inlines: &adoc::InlineNodes<'_>,
        span: Option<Span>,
    ) -> Vec<MarkupBlock> {
        match block.context {
            adoc::BlockContext::Listing | adoc::BlockContext::Literal => {
                vec![MarkupBlock::CodeBlock {
                    lang: adoc::AttrData::source_language(&block.meta.attrs).map(str::to_owned),
                    code: inline_plain_text(&self.inlines(inlines)),
                    span,
                }]
            }
            adoc::BlockContext::Passthrough
                if adoc::AttrData::has_str_positional(&block.meta.attrs, "stem") =>
            {
                vec![MarkupBlock::MathBlock {
                    source: MathSource {
                        notation: "asciidoc".to_owned(),
                        text: inline_plain_text(&self.inlines(inlines)),
                    },
                    span,
                }]
            }
            adoc::BlockContext::Passthrough => self
                .raw_from_span(
                    span,
                    "passthrough",
                    "asciidoc passthrough block is preserved raw",
                )
                .into_iter()
                .collect(),
            _ => {
                let content = self.inlines(inlines);
                if inline_plain_text(&content).trim().is_empty() {
                    Vec::new()
                } else {
                    vec![MarkupBlock::Paragraph { content, span }]
                }
            }
        }
    }

    fn empty_block(
        &mut self,
        block: &adoc::Block<'_>,
        empty: &adoc::EmptyMetadata<'_>,
        span: Option<Span>,
    ) -> Vec<MarkupBlock> {
        match empty {
            adoc::EmptyMetadata::Image { target, attrs, .. } => {
                let caption = block
                    .meta
                    .title()
                    .or_else(|| adoc::AttrData::positional_at(attrs, 0))
                    .map(|nodes| self.inlines(nodes))
                    .unwrap_or_default();
                vec![MarkupBlock::Figure {
                    src: target.src.to_string(),
                    caption,
                    span,
                }]
            }
            adoc::EmptyMetadata::DiscreteHeading { level, content, id } => {
                vec![MarkupBlock::Heading {
                    level: level.saturating_add(1).clamp(1, 6),
                    text: self.inlines(content),
                    id: id.as_ref().map(ToString::to_string),
                    span,
                }]
            }
            adoc::EmptyMetadata::Comment(source) => self
                .raw_text(
                    source.src.to_string(),
                    span,
                    "comment",
                    "asciidoc comment is preserved raw",
                )
                .into_iter()
                .collect(),
            adoc::EmptyMetadata::AudioVideo { .. } => self
                .raw_from_span(span, "media", "asciidoc media block is not semantic markup")
                .into_iter()
                .collect(),
            adoc::EmptyMetadata::None => Vec::new(),
        }
    }

    fn list_block(
        &mut self,
        variant: adoc::ListVariant,
        items: &[adoc::ListItem<'_>],
        span: Option<Span>,
    ) -> Vec<MarkupBlock> {
        let ordered = match variant {
            adoc::ListVariant::Ordered => true,
            adoc::ListVariant::Unordered => false,
            _ => {
                return self
                    .raw_from_span(span, "list", "asciidoc list variant is not supported")
                    .into_iter()
                    .collect();
            }
        };
        let items = items
            .iter()
            .map(|item| {
                let mut blocks = Vec::new();
                let principle = self.inlines(&item.principle);
                if !inline_plain_text(&principle).trim().is_empty() {
                    blocks.push(MarkupBlock::Paragraph {
                        content: principle,
                        span: span_from_loc(item.loc()),
                    });
                }
                blocks.extend(self.blocks_from_blocks(&item.blocks));
                blocks
            })
            .collect();
        vec![MarkupBlock::List {
            ordered,
            items,
            span,
        }]
    }

    fn row_cells(&mut self, row: &adoc::Row<'_>) -> Vec<Vec<Inline>> {
        row.cells
            .iter()
            .map(|cell| self.cell_inlines(cell))
            .collect()
    }

    fn cell_inlines(&mut self, cell: &adoc::Cell<'_>) -> Vec<Inline> {
        match &cell.content {
            adoc::CellContent::AsciiDoc(doc) => self.blocks_plain_inlines(&doc.content),
            adoc::CellContent::Literal(nodes) => {
                vec![Inline::Code(inline_plain_text(&self.inlines(nodes)))]
            }
            adoc::CellContent::Default(paras)
            | adoc::CellContent::Header(paras)
            | adoc::CellContent::Emphasis(paras)
            | adoc::CellContent::Monospace(paras)
            | adoc::CellContent::Strong(paras) => {
                let mut out = Vec::new();
                for (index, para) in paras.iter().enumerate() {
                    if index > 0 {
                        out.push(Inline::Text("\n".to_owned()));
                    }
                    out.extend(self.inlines(para));
                }
                out
            }
        }
    }

    fn blocks_plain_inlines(&mut self, content: &adoc::DocContent<'_>) -> Vec<Inline> {
        vec![Inline::Text(
            self.blocks_from_content(content)
                .iter()
                .map(block_plain_text)
                .collect::<Vec<_>>()
                .join("\n"),
        )]
    }

    fn inlines(&mut self, nodes: &adoc::InlineNodes<'_>) -> Vec<Inline> {
        let mut out = Vec::new();
        for node in nodes.iter() {
            if let Some(item) = self.inline(&node.content, node.loc) {
                out.push(item);
            }
        }
        out
    }

    fn inline(&mut self, node: &adoc::Inline<'_>, loc: adoc::SourceLocation) -> Option<Inline> {
        match node {
            adoc::Inline::Text(text) => Some(Inline::Text(text.to_string())),
            adoc::Inline::MultiCharWhitespace(_) | adoc::Inline::Newline => {
                Some(Inline::Text(" ".to_owned()))
            }
            adoc::Inline::LineBreak => Some(Inline::Text("\n".to_owned())),
            adoc::Inline::Quote(kind, children) => {
                let quote = match kind {
                    adoc::QuoteKind::Double => "\"",
                    adoc::QuoteKind::Single => "'",
                };
                Some(Inline::Text(format!(
                    "{quote}{}{quote}",
                    inline_plain_text(&self.inlines(children))
                )))
            }
            adoc::Inline::Span(kind, _, children) => match kind {
                adoc::SpanKind::Bold => Some(Inline::Strong(self.inlines(children))),
                adoc::SpanKind::Italic => Some(Inline::Emph(self.inlines(children))),
                adoc::SpanKind::LitMono | adoc::SpanKind::Mono => {
                    Some(Inline::Code(inline_plain_text(&self.inlines(children))))
                }
                adoc::SpanKind::Text => {
                    Some(Inline::Text(inline_plain_text(&self.inlines(children))))
                }
                _ => self.raw_inline_from_loc(loc, "span", "unsupported asciidoc span"),
            },
            adoc::Inline::InlinePassthru(children) => Some(Inline::Raw {
                backend: asciidoc_id(),
                text: inline_plain_text(&self.inlines(children)),
            }),
            adoc::Inline::Macro(macro_node) => self.macro_inline(macro_node, loc),
            adoc::Inline::SpecialChar(kind) => Some(Inline::Text(
                match kind {
                    adoc::SpecialCharKind::Ampersand => "&",
                    adoc::SpecialCharKind::LessThan => "<",
                    adoc::SpecialCharKind::GreaterThan => ">",
                }
                .to_owned(),
            )),
            adoc::Inline::Symbol(kind) => Some(Inline::Text(symbol_text(*kind).to_owned())),
            adoc::Inline::CurlyQuote(kind) => Some(Inline::Text(
                match kind {
                    adoc::CurlyKind::LeftDouble | adoc::CurlyKind::RightDouble => "\"",
                    adoc::CurlyKind::LeftSingle
                    | adoc::CurlyKind::RightSingle
                    | adoc::CurlyKind::LegacyImplicitApostrophe => "'",
                }
                .to_owned(),
            )),
            adoc::Inline::LineComment(text) => self.raw_inline(
                text.to_string(),
                "comment",
                "asciidoc line comment is preserved raw",
            ),
            adoc::Inline::Discarded
            | adoc::Inline::CalloutNum(_)
            | adoc::Inline::CalloutTuck(_)
            | adoc::Inline::InlineAnchor(_)
            | adoc::Inline::BiblioAnchor(_)
            | adoc::Inline::IndexTerm(_)
            | adoc::Inline::SpacedDashes(_, _) => None,
        }
    }

    fn macro_inline(
        &mut self,
        macro_node: &adoc::MacroNode<'_>,
        loc: adoc::SourceLocation,
    ) -> Option<Inline> {
        match macro_node {
            adoc::MacroNode::Link {
                scheme,
                target,
                attrs,
                ..
            } => {
                let target = link_target(*scheme, target);
                let label = attrs
                    .as_ref()
                    .and_then(|attrs| adoc::AttrData::positional_at(attrs, 0))
                    .map(|nodes| self.inlines(nodes))
                    .unwrap_or_else(|| vec![Inline::Text(target.clone())]);
                Some(Inline::Link { label, target })
            }
            adoc::MacroNode::Mailto {
                address, linktext, ..
            } => {
                let target = format!("mailto:{}", address.src);
                let label = linktext
                    .as_ref()
                    .map(|nodes| self.inlines(nodes))
                    .unwrap_or_else(|| vec![Inline::Text(address.src.to_string())]);
                Some(Inline::Link { label, target })
            }
            adoc::MacroNode::Xref {
                target, linktext, ..
            } => {
                let target = format!("#{}", target.src);
                let label = linktext
                    .as_ref()
                    .map(|nodes| self.inlines(nodes))
                    .unwrap_or_else(|| vec![Inline::Text(target.clone())]);
                Some(Inline::Link { label, target })
            }
            adoc::MacroNode::Plugin(plugin)
                if matches!(plugin.name.as_str(), "stem" | "latexmath" | "asciimath") =>
            {
                let text = plugin
                    .target
                    .as_ref()
                    .map(|target| target.src.to_string())
                    .unwrap_or_else(|| plugin.source.src.to_string());
                Some(Inline::Math(MathSource {
                    notation: "asciidoc".to_owned(),
                    text,
                }))
            }
            adoc::MacroNode::InlineImage { target, .. } => self.raw_inline(
                target.src.to_string(),
                "inline-image",
                "asciidoc inline image is preserved raw",
            ),
            adoc::MacroNode::Plugin(plugin) => self.raw_inline(
                plugin.source.src.to_string(),
                plugin.name.as_str(),
                "unsupported asciidoc macro is preserved raw",
            ),
            _ => self.raw_inline_from_loc(loc, "macro", "unsupported asciidoc macro"),
        }
    }

    fn record_directives(&mut self, directives: &[RawDirective]) {
        for directive in directives {
            self.fidelity.warnings.push(format!(
                "asciidoc {} directive is not resolved",
                directive.kind
            ));
            if self.preserve_raw {
                self.fidelity.preserved_raw.push(directive.text.clone());
                self.raw_blocks.push(MarkupBlock::Raw {
                    backend: asciidoc_id(),
                    text: directive.text.clone(),
                    span: Some(directive.span.clone()),
                });
            } else {
                self.fidelity.dropped.push(MarkupLoss {
                    path: directive.kind.clone(),
                    reason: "asciidoc directive omitted by raw-fragment policy".to_owned(),
                });
            }
        }
    }

    fn merge_directives(&mut self, blocks: Vec<MarkupBlock>) -> Vec<MarkupBlock> {
        if self.raw_blocks.is_empty() {
            return blocks;
        }
        let mut entries = blocks
            .into_iter()
            .enumerate()
            .map(|(index, block)| (block_start(&block).unwrap_or(usize::MAX), index, block))
            .collect::<Vec<_>>();
        let offset = entries.len();
        entries.extend(self.raw_blocks.drain(..).enumerate().map(|(index, block)| {
            (
                block_start(&block).unwrap_or(usize::MAX),
                offset + index,
                block,
            )
        }));
        entries.sort_by_key(|(start, index, _)| (*start, *index));
        entries.into_iter().map(|(_, _, block)| block).collect()
    }

    fn raw_from_span(
        &mut self,
        span: Option<Span>,
        path: &str,
        reason: &str,
    ) -> Option<MarkupBlock> {
        let text = span
            .as_ref()
            .and_then(|span| self.input.get(span.start..span.end))
            .unwrap_or_default()
            .to_owned();
        self.raw_text(text, span, path, reason)
    }

    fn raw_text(
        &mut self,
        text: String,
        span: Option<Span>,
        path: &str,
        reason: &str,
    ) -> Option<MarkupBlock> {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(text.clone());
            Some(MarkupBlock::Raw {
                backend: asciidoc_id(),
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

    fn raw_inline_from_loc(
        &mut self,
        loc: adoc::SourceLocation,
        path: &str,
        reason: &str,
    ) -> Option<Inline> {
        let text = self
            .input
            .get(usize::try_from(loc.start).ok()?..usize::try_from(loc.end).ok()?)
            .unwrap_or_default()
            .to_owned();
        self.raw_inline(text, path, reason)
    }

    fn raw_inline(&mut self, text: String, path: &str, reason: &str) -> Option<Inline> {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(text.clone());
            Some(Inline::Raw {
                backend: asciidoc_id(),
                text,
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
