//! Scanner helpers for the safe LaTeX backend.

use crate::backend::{MarkupFidelity, MarkupLoss};
use crate::markup::{Inline, MarkupBlock, MathSource, Span, SpanState};

use super::{BracedArg, Environment, latex_id};

pub(super) fn parse_inlines(
    text: &str,
    preserve_raw: bool,
    fidelity: &mut MarkupFidelity,
) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut cursor = 0;
    while cursor < text.len() {
        if let Some(close) = text[cursor..]
            .starts_with('$')
            .then(|| find_unescaped(text, cursor + 1, "$"))
            .flatten()
        {
            flush_plain(&mut out, &mut plain);
            out.push(Inline::Math(tex_math(&text[cursor + 1..close])));
            cursor = close + 1;
            continue;
        }
        if let Some((inline, next)) = inline_command(text, cursor, preserve_raw, fidelity) {
            flush_plain(&mut out, &mut plain);
            if let Some(inline) = inline {
                out.push(inline);
            }
            cursor = next;
            continue;
        }
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        if ch == '\\'
            && let Some((escaped, next)) = escaped_char(text, cursor)
        {
            plain.push(escaped);
            cursor = next;
            continue;
        }
        plain.push(ch);
        cursor += ch.len_utf8();
    }
    flush_plain(&mut out, &mut plain);
    out
}

fn inline_command(
    text: &str,
    start: usize,
    preserve_raw: bool,
    fidelity: &mut MarkupFidelity,
) -> Option<(Option<Inline>, usize)> {
    if let Some(arg) = command_arg_at(text, start, "\\emph") {
        return Some((
            Some(Inline::Emph(parse_inlines(
                arg.text(text),
                preserve_raw,
                fidelity,
            ))),
            arg.next,
        ));
    }
    if let Some(arg) = command_arg_at(text, start, "\\textbf") {
        return Some((
            Some(Inline::Strong(parse_inlines(
                arg.text(text),
                preserve_raw,
                fidelity,
            ))),
            arg.next,
        ));
    }
    if let Some(arg) = command_arg_at(text, start, "\\texttt") {
        return Some((Some(Inline::Code(unescape_text(arg.text(text)))), arg.next));
    }
    if let Some(first) = command_arg_at(text, start, "\\href")
        && let Some(second) = braced_arg_at(text, skip_ws(text, first.next))
    {
        return Some((
            Some(Inline::Link {
                label: parse_inlines(second.text(text), preserve_raw, fidelity),
                target: unescape_text(first.text(text)),
            }),
            second.next,
        ));
    }
    if let Some(arg) = command_arg_at(text, start, "\\cite") {
        return Some((
            raw_inline(&text[start..arg.next], "citation", preserve_raw, fidelity),
            arg.next,
        ));
    }
    if text[start..].starts_with('\\') {
        let end = command_arg_end(text, start).max(command_name_end(text, start));
        if end > start + 1 {
            return Some((
                raw_inline(&text[start..end], "inline", preserve_raw, fidelity),
                end,
            ));
        }
    }
    None
}

fn raw_inline(
    text: &str,
    path: &str,
    preserve_raw: bool,
    fidelity: &mut MarkupFidelity,
) -> Option<Inline> {
    if preserve_raw {
        fidelity.preserved_raw.push(text.to_owned());
        Some(Inline::Raw {
            backend: latex_id(),
            text: text.to_owned(),
        })
    } else {
        fidelity.dropped.push(MarkupLoss {
            path: path.to_owned(),
            reason: "raw latex inline fragment omitted".to_owned(),
        });
        None
    }
}

fn flush_plain(out: &mut Vec<Inline>, plain: &mut String) {
    if !plain.is_empty() {
        out.push(Inline::Text(std::mem::take(plain)));
    }
}

fn escaped_char(input: &str, start: usize) -> Option<(char, usize)> {
    let next = input[start + 1..].chars().next()?;
    matches!(next, '\\' | '{' | '}' | '%' | '&' | '$' | '_' | '#')
        .then_some((next, start + 1 + next.len_utf8()))
}

pub(super) fn command_arg_anywhere(input: &str, command: &str) -> Option<BracedArg> {
    let mut offset = 0;
    while let Some(found) = input[offset..].find(command) {
        let start = offset + found;
        if let Some(arg) = command_arg_at(input, start, command) {
            return Some(arg);
        }
        offset = start + command.len();
    }
    None
}

pub(super) fn command_arg_at(input: &str, start: usize, command: &str) -> Option<BracedArg> {
    if !starts_command(input, start, command) {
        return None;
    }
    let cursor = skip_ws(input, start + command.len());
    let cursor = if input[cursor..].starts_with('[') {
        bracket_arg_at(input, cursor)
            .map(|arg| skip_ws(input, arg.next))
            .unwrap_or(cursor)
    } else {
        cursor
    };
    braced_arg_at(input, cursor)
}

pub(super) fn command_arg_end(input: &str, start: usize) -> usize {
    let end = command_name_end(input, start);
    let cursor = skip_ws(input, end);
    braced_arg_at(input, cursor).map_or(end, |arg| arg.next)
}

fn command_name_end(input: &str, start: usize) -> usize {
    if !input[start..].starts_with('\\') {
        return start;
    }
    let mut cursor = start + 1;
    while cursor < input.len() {
        let Some(ch) = input[cursor..].chars().next() else {
            break;
        };
        if !ch.is_ascii_alphabetic() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor.max(start + 1)
}

fn braced_arg_at(input: &str, open: usize) -> Option<BracedArg> {
    grouped_arg_at(input, open, '{', '}')
}

fn bracket_arg_at(input: &str, open: usize) -> Option<BracedArg> {
    grouped_arg_at(input, open, '[', ']')
}

fn grouped_arg_at(input: &str, open: usize, left: char, right: char) -> Option<BracedArg> {
    if !input[open..].starts_with(left) {
        return None;
    }
    let mut depth = 0usize;
    let mut escaped = false;
    for (rel, ch) in input[open..].char_indices() {
        let idx = open + rel;
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == left {
            depth += 1;
            continue;
        }
        if ch == right {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(BracedArg {
                    content_start: open + left.len_utf8(),
                    content_end: idx,
                    next: idx + right.len_utf8(),
                });
            }
        }
    }
    None
}

pub(super) fn begin_environment_at<'a>(input: &'a str, start: usize) -> Option<Environment<'a>> {
    let begin = command_arg_at(input, start, "\\begin")?;
    let name = begin.text(input).trim();
    let mut content_start = skip_ws(input, begin.next);
    if name == "tabular"
        && let Some(spec) = braced_arg_at(input, content_start)
    {
        content_start = skip_ws(input, spec.next);
    }
    let marker = format!("\\end{{{name}}}");
    let content_end = input[content_start..]
        .find(&marker)
        .map(|found| content_start + found)?;
    Some(Environment {
        name,
        start,
        content_start,
        content_end,
        after_end: content_end + marker.len(),
    })
}

pub(super) fn split_items(input: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut current: Option<usize> = None;
    let mut cursor = 0;
    while let Some(found) = input[cursor..].find("\\item") {
        let start = cursor + found;
        if !starts_command(input, start, "\\item") {
            cursor = start + "\\item".len();
            continue;
        }
        if let Some(item_start) = current {
            items.push(&input[item_start..start]);
        }
        current = Some(skip_ws(input, start + "\\item".len()));
        cursor = start + "\\item".len();
    }
    if let Some(item_start) = current {
        items.push(&input[item_start..]);
    }
    items
}

pub(super) fn table_rows(input: &str) -> Vec<Vec<&str>> {
    split_table_rows(input)
        .into_iter()
        .filter_map(|row| {
            let row = row.trim();
            if row.is_empty() || row == "\\hline" {
                return None;
            }
            let cells = split_cells(row)
                .into_iter()
                .map(str::trim)
                .filter(|cell| !cell.is_empty() && *cell != "\\hline")
                .collect::<Vec<_>>();
            (!cells.is_empty()).then_some(cells)
        })
        .collect()
}

fn split_table_rows(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = input.as_bytes();
    let mut cursor = 0;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'\\' && bytes[cursor + 1] == b'\\' && !is_escaped(input, cursor) {
            out.push(&input[start..cursor]);
            cursor += 2;
            start = cursor;
            continue;
        }
        cursor += 1;
    }
    if start < input.len() {
        out.push(&input[start..]);
    }
    out
}

fn split_cells(row: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (idx, ch) in row.char_indices() {
        if ch == '&' && !is_escaped(row, idx) {
            out.push(&row[start..idx]);
            start = idx + 1;
        }
    }
    out.push(&row[start..]);
    out
}

pub(super) fn paragraph_line(input: &str, start: usize, end: usize) -> Option<(&str, usize)> {
    let (text, text_end) = strip_comment(&input[start..end]);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some((trimmed, start + text_end))
}

fn strip_comment(input: &str) -> (&str, usize) {
    for (idx, ch) in input.char_indices() {
        if ch == '%' && !is_escaped(input, idx) {
            return (&input[..idx], idx);
        }
    }
    (input, input.len())
}

pub(super) fn find_unescaped(input: &str, start: usize, needle: &str) -> Option<usize> {
    let mut cursor = start;
    while let Some(found) = input[cursor..].find(needle) {
        let idx = cursor + found;
        if !is_escaped(input, idx) {
            return Some(idx);
        }
        cursor = idx + needle.len();
    }
    None
}

fn is_escaped(input: &str, idx: usize) -> bool {
    let mut slashes = 0usize;
    for byte in input[..idx].bytes().rev() {
        if byte == b'\\' {
            slashes += 1;
        } else {
            break;
        }
    }
    slashes % 2 == 1
}

pub(super) fn starts_command(input: &str, start: usize, command: &str) -> bool {
    if !input[start..].starts_with(command) {
        return false;
    }
    input[start + command.len()..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphabetic())
}

pub(super) fn line_bounds(input: &str, start: usize) -> (usize, usize) {
    input[start..]
        .find('\n')
        .map_or((input.len(), input.len()), |pos| {
            (start + pos, start + pos + 1)
        })
}

pub(super) fn after_line(input: &str, pos: usize) -> usize {
    input[pos..]
        .find('\n')
        .map_or(input.len(), |found| pos + found + 1)
}

fn skip_ws(input: &str, mut cursor: usize) -> usize {
    while cursor < input.len() {
        let Some(ch) = input[cursor..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

pub(super) fn trim_edge_newlines(input: &str) -> &str {
    input.trim_matches(['\r', '\n'])
}

pub(super) fn tex_math(text: &str) -> MathSource {
    MathSource {
        notation: "tex".to_owned(),
        text: text.to_owned(),
    }
}

pub(super) fn unescape_text(input: &str) -> String {
    let mut out = String::new();
    let mut cursor = 0;
    while cursor < input.len() {
        if let Some((ch, next)) = escaped_char(input, cursor) {
            out.push(ch);
            cursor = next;
            continue;
        }
        let Some(ch) = input[cursor..].chars().next() else {
            break;
        };
        out.push(ch);
        cursor += ch.len_utf8();
    }
    out
}

pub(super) fn offset_block_span(block: &mut MarkupBlock, offset: usize) {
    match block {
        MarkupBlock::Heading { span, .. }
        | MarkupBlock::Paragraph { span, .. }
        | MarkupBlock::CodeBlock { span, .. }
        | MarkupBlock::MathBlock { span, .. }
        | MarkupBlock::Quote { span, .. }
        | MarkupBlock::List { span, .. }
        | MarkupBlock::Table { span, .. }
        | MarkupBlock::Figure { span, .. }
        | MarkupBlock::Raw { span, .. } => {
            if let Some(span) = span {
                span.start += offset;
                span.end += offset;
            }
        }
    }
    match block {
        MarkupBlock::Quote { blocks, .. } => {
            for block in blocks {
                offset_block_span(block, offset);
            }
        }
        MarkupBlock::List { items, .. } => {
            for item in items {
                for block in item {
                    offset_block_span(block, offset);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn span(start: usize, end: usize) -> Span {
    Span {
        start,
        end,
        state: SpanState::Preserved,
    }
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
