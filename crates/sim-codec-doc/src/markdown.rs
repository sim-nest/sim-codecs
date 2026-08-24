//! Markdown backend implementation over `pulldown-cmark`.

use std::collections::BTreeMap;
use std::fmt;
use std::mem;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::Value as JsonValue;
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::CodecId;

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

/// Attribute spelling accepted around a Markdown document body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttributeEnvelope {
    /// Do not read or write document attributes.
    #[default]
    None,
    /// A canonical JSON object between `---json` and `---` lines.
    JsonFrontMatter,
    /// Consecutive `key:: JSON-value` lines followed by a blank line.
    DoubleColon,
}

/// Link spelling used by a Markdown dialect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkDialect {
    /// Standard `[label](target)` links.
    #[default]
    CommonMark,
    /// Bounded `[[target]]` and `[[target|label]]` links.
    WikiLink,
}

/// Generic, bounded Markdown syntax policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkdownDialect {
    /// Attribute envelope syntax.
    pub attributes: AttributeEnvelope,
    /// Link syntax.
    pub links: LinkDialect,
    /// Maximum encoded attribute-envelope bytes.
    pub max_attribute_bytes: usize,
    /// Maximum number of attributes.
    pub max_attributes: usize,
    /// Maximum bytes in one wikilink.
    pub max_wikilink_bytes: usize,
}

impl Default for MarkdownDialect {
    fn default() -> Self {
        Self {
            attributes: AttributeEnvelope::None,
            links: LinkDialect::CommonMark,
            max_attribute_bytes: 64 * 1024,
            max_attributes: 256,
            max_wikilink_bytes: 4 * 1024,
        }
    }
}

/// Markdown backend configured with an opt-in generic dialect.
#[derive(Clone, Debug)]
pub struct DialectMarkdownBackend {
    dialect: MarkdownDialect,
}

impl DialectMarkdownBackend {
    /// Construct a backend after validating all resource bounds.
    pub fn new(dialect: MarkdownDialect) -> Result<Self, MarkupError> {
        if dialect.max_attribute_bytes == 0
            || dialect.max_attributes == 0
            || dialect.max_wikilink_bytes == 0
        {
            return Err(MarkupError::InvalidDocument(
                "Markdown dialect bounds must be non-zero".to_owned(),
            ));
        }
        Ok(Self { dialect })
    }
}

impl MarkupBackend for MarkdownBackend {
    fn id(&self) -> BackendId {
        markdown_id()
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        DialectMarkdownBackend::new(MarkdownDialect::default())
            .expect("default bounds")
            .decode(input, opts)
    }

    fn encode(
        &self,
        doc: &MarkupDoc,
        opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        DialectMarkdownBackend::new(MarkdownDialect::default())
            .expect("default bounds")
            .encode(doc, opts)
    }
}

impl MarkupBackend for DialectMarkdownBackend {
    fn id(&self) -> BackendId {
        markdown_id()
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        let (attrs, body_start) = decode_attributes(input, self.dialect)?;
        let body = &input[body_start..];
        let events = Parser::new_ext(body, markdown_options())
            .into_offset_iter()
            .map(|(event, range)| {
                (
                    event.into_static(),
                    range.start + body_start..range.end + body_start,
                )
            })
            .collect();
        let mut parser = MarkdownParser::new(input, events, opts);
        let mut blocks = parser.parse_blocks_until(|_| false);
        if self.dialect.links == LinkDialect::WikiLink {
            rewrite_wikilinks(&mut blocks, self.dialect.max_wikilink_bytes)?;
        }
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
                attrs,
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
        let mut encoder = MarkdownEncoder::new(opts, self.dialect.links);
        let mut source = encode_attributes(&doc.attrs, self.dialect)?;
        source.push_str(&encoder.write_doc(doc));
        if opts.fail_on_loss && !encoder.fidelity.dropped.is_empty() {
            return Err(MarkupError::Encode(format!(
                "markdown encode dropped {} raw fragment(s)",
                encoder.fidelity.dropped.len()
            )));
        }
        Ok((source, encoder.fidelity))
    }
}

fn decode_attributes(
    input: &str,
    dialect: MarkdownDialect,
) -> Result<(BTreeMap<String, sim_kernel::Expr>, usize), MarkupError> {
    match dialect.attributes {
        AttributeEnvelope::None => Ok((BTreeMap::new(), 0)),
        AttributeEnvelope::JsonFrontMatter => {
            if !input.starts_with("---json\n") {
                return Ok((BTreeMap::new(), 0));
            }
            let end = input[8..]
                .find("\n---\n")
                .ok_or_else(|| MarkupError::Decode("unterminated JSON front matter".to_owned()))?
                + 8;
            if end > dialect.max_attribute_bytes {
                return Err(MarkupError::Decode(
                    "JSON front matter exceeds dialect byte bound".to_owned(),
                ));
            }
            let pairs = parse_json_object_pairs(&input[8..end])?;
            attrs_from_pairs(pairs, dialect, end + 5)
        }
        AttributeEnvelope::DoubleColon => {
            let Some(end) = input.find("\n\n") else {
                return Ok((BTreeMap::new(), 0));
            };
            let prelude = &input[..end];
            if prelude.is_empty() || !prelude.lines().all(|line| line.contains("::")) {
                return Ok((BTreeMap::new(), 0));
            }
            if end > dialect.max_attribute_bytes {
                return Err(MarkupError::Decode(
                    "property prelude exceeds dialect byte bound".to_owned(),
                ));
            }
            let mut pairs = Vec::new();
            for line in prelude.lines() {
                let (key, value) = line.split_once("::").expect("checked above");
                let key = key.trim();
                validate_key(key)?;
                let value = serde_json::from_str(value.trim()).map_err(|error| {
                    MarkupError::Decode(format!("invalid JSON property {key:?}: {error}"))
                })?;
                pairs.push((key.to_owned(), value));
            }
            attrs_from_pairs(pairs, dialect, end + 2)
        }
    }
}

fn attrs_from_pairs(
    pairs: Vec<(String, JsonValue)>,
    dialect: MarkdownDialect,
    body_start: usize,
) -> Result<(BTreeMap<String, sim_kernel::Expr>, usize), MarkupError> {
    if pairs.len() > dialect.max_attributes {
        return Err(MarkupError::Decode(
            "attribute count exceeds dialect bound".to_owned(),
        ));
    }
    let mut attrs = BTreeMap::new();
    for (key, value) in pairs {
        validate_key(&key)?;
        let mut budget = DecodeBudget::new(DecodeLimits::default());
        let expr =
            sim_codec_json::json_to_expr(CodecId(0), &value, &mut budget, 0).map_err(|error| {
                MarkupError::Decode(format!("invalid Expr attribute {key:?}: {error}"))
            })?;
        if attrs.insert(key.clone(), expr).is_some() {
            return Err(MarkupError::Decode(format!(
                "duplicate attribute key {key:?}"
            )));
        }
    }
    Ok((attrs, body_start))
}

fn encode_attributes(
    attrs: &BTreeMap<String, sim_kernel::Expr>,
    dialect: MarkdownDialect,
) -> Result<String, MarkupError> {
    if dialect.attributes == AttributeEnvelope::None || attrs.is_empty() {
        return Ok(String::new());
    }
    if attrs.len() > dialect.max_attributes {
        return Err(MarkupError::Encode(
            "attribute count exceeds dialect bound".to_owned(),
        ));
    }
    let mut object = serde_json::Map::new();
    for (key, expr) in attrs {
        validate_key(key).map_err(|error| MarkupError::Encode(error.to_string()))?;
        object.insert(key.clone(), sim_codec_json::expr_to_json(expr));
    }
    let out = match dialect.attributes {
        AttributeEnvelope::None => String::new(),
        AttributeEnvelope::JsonFrontMatter => format!(
            "---json\n{}\n---\n",
            serde_json::to_string(&JsonValue::Object(object))
                .map_err(|e| MarkupError::Encode(e.to_string()))?
        ),
        AttributeEnvelope::DoubleColon => {
            let mut out = String::new();
            for (key, value) in object {
                out.push_str(&key);
                out.push_str(":: ");
                out.push_str(
                    &serde_json::to_string(&value)
                        .map_err(|e| MarkupError::Encode(e.to_string()))?,
                );
                out.push('\n');
            }
            out.push('\n');
            out
        }
    };
    if out.len() > dialect.max_attribute_bytes {
        return Err(MarkupError::Encode(
            "attribute envelope exceeds dialect byte bound".to_owned(),
        ));
    }
    Ok(out)
}

fn validate_key(key: &str) -> Result<(), MarkupError> {
    if key.is_empty() || key.contains(['\n', '\r', '\0']) {
        return Err(MarkupError::InvalidDocument(
            "attribute keys must be non-empty, single-line, and NUL-free".to_owned(),
        ));
    }
    Ok(())
}

struct PairVisitor;
impl<'de> Visitor<'de> for PairVisitor {
    type Value = Vec<(String, JsonValue)>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut pairs = Vec::new();
        while let Some(pair) = map.next_entry()? {
            pairs.push(pair);
        }
        Ok(pairs)
    }
}

fn parse_json_object_pairs(source: &str) -> Result<Vec<(String, JsonValue)>, MarkupError> {
    let mut decoder = serde_json::Deserializer::from_str(source);
    let pairs = decoder
        .deserialize_map(PairVisitor)
        .map_err(|e| MarkupError::Decode(format!("invalid JSON front matter: {e}")))?;
    decoder
        .end()
        .map_err(|e| MarkupError::Decode(format!("trailing JSON front matter: {e}")))?;
    Ok(pairs)
}

fn rewrite_wikilinks(blocks: &mut [MarkupBlock], max_bytes: usize) -> Result<(), MarkupError> {
    for block in blocks {
        match block {
            MarkupBlock::Heading { text, .. } => rewrite_inline_list(text, max_bytes)?,
            MarkupBlock::Paragraph { content, .. } => rewrite_inline_list(content, max_bytes)?,
            MarkupBlock::Quote { blocks, .. } => rewrite_wikilinks(blocks, max_bytes)?,
            MarkupBlock::List { items, .. } => {
                for blocks in items {
                    rewrite_wikilinks(blocks, max_bytes)?;
                }
            }
            MarkupBlock::Table { header, rows, .. } => {
                for cell in header {
                    rewrite_inline_list(cell, max_bytes)?;
                }
                for row in rows {
                    for cell in row {
                        rewrite_inline_list(cell, max_bytes)?;
                    }
                }
            }
            MarkupBlock::Figure { caption, .. } => rewrite_inline_list(caption, max_bytes)?,
            _ => {}
        }
    }
    Ok(())
}

fn rewrite_inline_list(items: &mut Vec<Inline>, max_bytes: usize) -> Result<(), MarkupError> {
    let mut rewritten = Vec::new();
    let mut combined = Vec::new();
    for item in mem::take(items) {
        if let Inline::Text(text) = item {
            if let Some(Inline::Text(previous)) = combined.last_mut() {
                previous.push_str(&text);
            } else {
                combined.push(Inline::Text(text));
            }
        } else {
            combined.push(item);
        }
    }
    for mut item in combined {
        match &mut item {
            Inline::Text(text) => rewritten.extend(parse_wikilink_text(text, max_bytes)?),
            Inline::Emph(children) | Inline::Strong(children) => {
                rewrite_inline_list(children, max_bytes)?;
                rewritten.push(item);
            }
            _ => rewritten.push(item),
        }
    }
    *items = rewritten;
    Ok(())
}

fn parse_wikilink_text(text: &str, max_bytes: usize) -> Result<Vec<Inline>, MarkupError> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        if start > 0 {
            out.push(Inline::Text(rest[..start].to_owned()));
        }
        let after = &rest[start + 2..];
        let end = find_unescaped(after, "]]", max_bytes)
            .ok_or_else(|| MarkupError::Decode("malformed or oversized wikilink".to_owned()))?;
        let body = &after[..end];
        if body.contains(['\n', '\r', '\0']) {
            return Err(MarkupError::Decode(
                "wikilinks must be single-line and NUL-free".to_owned(),
            ));
        }
        let split = find_unescaped(body, "|", max_bytes);
        let (target, label) = split.map_or((body, body), |at| (&body[..at], &body[at + 1..]));
        let target = unescape_wikilink(target)?;
        let label = unescape_wikilink(label)?;
        if target.is_empty() {
            return Err(MarkupError::Decode("wikilink target is empty".to_owned()));
        }
        out.push(Inline::Link {
            label: vec![Inline::Text(label)],
            target,
        });
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        out.push(Inline::Text(rest.to_owned()));
    }
    Ok(out)
}

fn find_unescaped(text: &str, needle: &str, max_bytes: usize) -> Option<usize> {
    let limit = text.len().min(max_bytes + needle.len());
    text[..limit]
        .match_indices(needle)
        .find(|(at, _)| {
            text[..*at]
                .bytes()
                .rev()
                .take_while(|b| *b == b'\\')
                .count()
                % 2
                == 0
        })
        .map(|(at, _)| at)
}

fn unescape_wikilink(text: &str) -> Result<String, MarkupError> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let Some(next) = chars.next() else {
                return Err(MarkupError::Decode("trailing wikilink escape".to_owned()));
            };
            if !matches!(next, '\\' | '|' | ']') {
                return Err(MarkupError::Decode(
                    "unsupported wikilink escape".to_owned(),
                ));
            }
            out.push(next);
        } else {
            out.push(ch);
        }
    }
    percent_unescape_wikilink(&out)
}

fn percent_unescape_wikilink(text: &str) -> Result<String, MarkupError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(MarkupError::Decode(
                    "truncated wikilink percent escape".to_owned(),
                ));
            }
            let value = match &text[index..index + 3] {
                "%25" => b'%',
                "%5C" | "%5c" => b'\\',
                "%7C" | "%7c" => b'|',
                "%5D" | "%5d" => b']',
                _ => {
                    return Err(MarkupError::Decode(
                        "unsupported wikilink percent escape".to_owned(),
                    ));
                }
            };
            out.push(value);
            index += 3;
        } else {
            let ch = text[index..].chars().next().expect("valid UTF-8");
            let mut encoded = [0; 4];
            out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            index += ch.len_utf8();
        }
    }
    String::from_utf8(out).map_err(|_| MarkupError::Decode("invalid wikilink UTF-8".to_owned()))
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
    Span {
        start,
        end,
        state: crate::SpanState::Preserved,
    }
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
