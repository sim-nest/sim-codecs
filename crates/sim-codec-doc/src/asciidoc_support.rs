//! Support helpers shared by the AsciiDoc reader and writer.

use std::borrow::Cow;

use asciidork_ast as adoc;

use crate::markup::{BackendId, Inline, MarkupBlock, Span, SpanState};

#[derive(Clone, Debug)]
pub(super) struct RawDirective {
    pub(super) kind: String,
    pub(super) text: String,
    pub(super) span: Span,
}

pub(super) fn raw_directives(input: &str) -> Vec<RawDirective> {
    let mut out = Vec::new();
    let mut start = 0;
    for line in input.split_inclusive('\n') {
        let end = start + line.len();
        let trimmed_newline = line.trim_end_matches(['\r', '\n']);
        let leading = trimmed_newline.len() - trimmed_newline.trim_start().len();
        let trimmed = trimmed_newline.trim_start();
        if let Some(kind) = directive_kind(trimmed) {
            out.push(RawDirective {
                kind: kind.to_owned(),
                text: trimmed_newline.to_owned(),
                span: span(start + leading, start + trimmed_newline.len()),
            });
        }
        start = end;
    }
    out
}

pub(super) fn mask_directives<'a>(input: &'a str, directives: &[RawDirective]) -> Cow<'a, str> {
    if directives.is_empty() {
        return Cow::Borrowed(input);
    }
    let mut bytes = input.as_bytes().to_vec();
    for directive in directives {
        for byte in bytes
            .iter_mut()
            .take(directive.span.end)
            .skip(directive.span.start)
        {
            *byte = b' ';
        }
    }
    Cow::Owned(String::from_utf8(bytes).expect("masked source remains utf-8"))
}

pub(super) fn span_from_multi(loc: &adoc::MultiSourceLocation) -> Option<Span> {
    loc.coalesce().and_then(span_from_loc)
}

pub(super) fn span_from_loc(loc: adoc::SourceLocation) -> Option<Span> {
    if loc.include_depth == 0 && loc.start < loc.end {
        Some(span(
            usize::try_from(loc.start).ok()?,
            usize::try_from(loc.end).ok()?,
        ))
    } else {
        None
    }
}

pub(super) fn block_start(block: &MarkupBlock) -> Option<usize> {
    match block {
        MarkupBlock::Heading { span, .. }
        | MarkupBlock::Paragraph { span, .. }
        | MarkupBlock::CodeBlock { span, .. }
        | MarkupBlock::MathBlock { span, .. }
        | MarkupBlock::Quote { span, .. }
        | MarkupBlock::List { span, .. }
        | MarkupBlock::Table { span, .. }
        | MarkupBlock::Figure { span, .. }
        | MarkupBlock::Raw { span, .. } => span.as_ref().map(|span| span.start),
    }
}

pub(super) fn block_plain_text(block: &MarkupBlock) -> String {
    match block {
        MarkupBlock::Heading { text, .. } => inline_plain_text(text),
        MarkupBlock::Paragraph { content, .. } => inline_plain_text(content),
        MarkupBlock::CodeBlock { code, .. } => code.clone(),
        MarkupBlock::MathBlock { source, .. } => source.text.clone(),
        MarkupBlock::Quote { blocks, .. } => blocks
            .iter()
            .map(block_plain_text)
            .collect::<Vec<_>>()
            .join("\n"),
        MarkupBlock::List { items, .. } => items
            .iter()
            .flat_map(|item| item.iter().map(block_plain_text))
            .collect::<Vec<_>>()
            .join("\n"),
        MarkupBlock::Table { header, rows, .. } => header
            .iter()
            .chain(rows.iter().flat_map(|row| row.iter()))
            .map(|cell| inline_plain_text(cell))
            .collect::<Vec<_>>()
            .join(" "),
        MarkupBlock::Figure { src, caption, .. } => {
            format!("{} {}", src, inline_plain_text(caption))
        }
        MarkupBlock::Raw { text, .. } => text.clone(),
    }
}

pub(super) fn link_target(
    scheme: Option<adoc::UrlScheme>,
    target: &adoc::SourceString<'_>,
) -> String {
    let target = target.src.to_string();
    match scheme {
        Some(adoc::UrlScheme::Https) => format!("https://{target}"),
        Some(adoc::UrlScheme::Http) => format!("http://{target}"),
        Some(adoc::UrlScheme::Ftp) => format!("ftp://{target}"),
        Some(adoc::UrlScheme::Irc) => format!("irc://{target}"),
        Some(adoc::UrlScheme::Mailto) => format!("mailto:{target}"),
        Some(adoc::UrlScheme::File) => format!("file:{target}"),
        None => target,
    }
}

pub(super) fn symbol_text(kind: adoc::SymbolKind) -> &'static str {
    match kind {
        adoc::SymbolKind::Copyright => "(C)",
        adoc::SymbolKind::Registered => "(R)",
        adoc::SymbolKind::Trademark => "(TM)",
        adoc::SymbolKind::EmDash => "--",
        adoc::SymbolKind::TripleDash => "---",
        adoc::SymbolKind::Ellipsis => "...",
        adoc::SymbolKind::SingleRightArrow => "->",
        adoc::SymbolKind::DoubleRightArrow => "=>",
        adoc::SymbolKind::SingleLeftArrow => "<-",
        adoc::SymbolKind::DoubleLeftArrow => "<=",
    }
}

pub(super) fn asciidoc_id() -> BackendId {
    BackendId::new("asciidoc")
}

pub(super) fn inline_plain_text(items: &[Inline]) -> String {
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

fn directive_kind(line: &str) -> Option<&'static str> {
    [
        ("include::", "include"),
        ("ifdef::", "conditional"),
        ("ifndef::", "conditional"),
        ("ifeval::", "conditional"),
        ("elsif::", "conditional"),
        ("else::", "conditional"),
        ("endif::", "conditional"),
    ]
    .into_iter()
    .find_map(|(prefix, kind)| line.starts_with(prefix).then_some(kind))
}

fn span(start: usize, end: usize) -> Span {
    Span {
        start,
        end,
        state: SpanState::Preserved,
    }
}
