//! Lexer for Lisp source: scans text into delimiter, quote, dispatch, atom,
//! string, and byte tokens with span and trivia bookkeeping for round-tripping.

use proc_macro2::Delimiter;
use sim_codec::DecodeBudget;
use sim_kernel::{Origin, Result, SourceId, Span, Trivia};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LispTokenKind {
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    Quote,
    Dispatch,
    Atom(String),
    String(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LispToken {
    pub(crate) kind: LispTokenKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) leading_trivia: Vec<Trivia>,
}

pub(crate) fn lex_lisp_tokens(
    codec: sim_kernel::CodecId,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Vec<LispToken>> {
    lex_lisp_tokens_inner(codec, source, budget, true)
}

pub(crate) fn lex_lisp_tokens_without_trivia(
    codec: sim_kernel::CodecId,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Vec<LispToken>> {
    lex_lisp_tokens_inner(codec, source, budget, false)
}

fn lex_lisp_tokens_inner(
    codec: sim_kernel::CodecId,
    source: &str,
    budget: &mut DecodeBudget,
    preserve_trivia: bool,
) -> Result<Vec<LispToken>> {
    let chars = source.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    let mut tokens = Vec::new();

    while index < chars.len() {
        let mut leading_trivia = Vec::new();
        while index < chars.len() {
            let (start, ch) = chars[index];
            if ch.is_whitespace() {
                let ws_start = start;
                index += 1;
                while index < chars.len() && chars[index].1.is_whitespace() {
                    index += 1;
                }
                let ws_end = chars
                    .get(index)
                    .map(|(offset, _)| *offset)
                    .unwrap_or(source.len());
                if preserve_trivia {
                    budget.add_trivia(codec)?;
                    leading_trivia.push(Trivia::Whitespace(source[ws_start..ws_end].to_owned()));
                }
                continue;
            }
            if ch == ';' {
                let comment_start = start;
                index += 1;
                while index < chars.len() && chars[index].1 != '\n' {
                    index += 1;
                }
                let comment_end = chars
                    .get(index)
                    .map(|(offset, _)| *offset)
                    .unwrap_or(source.len());
                if preserve_trivia {
                    budget.add_trivia(codec)?;
                    leading_trivia.push(Trivia::LineComment(
                        source[comment_start..comment_end].to_owned(),
                    ));
                }
                continue;
            }
            break;
        }
        if index >= chars.len() {
            break;
        }
        let (start, ch) = chars[index];
        match ch {
            '(' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::OpenParen,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            ')' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::CloseParen,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            '[' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::OpenBracket,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            ']' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::CloseBracket,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            '{' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::OpenBrace,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            '}' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::CloseBrace,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            '\'' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::Quote,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            '#' => {
                tokens.push(LispToken {
                    kind: LispTokenKind::Dispatch,
                    start,
                    end: start + ch.len_utf8(),
                    leading_trivia,
                });
                index += 1;
            }
            '"' => {
                let end = scan_lisp_string_end(source, &chars, index)?;
                let raw = &source[start..end];
                let parsed = super::forms::parse_string_literal(codec, raw)?;
                budget.check_string_bytes(codec, parsed.len())?;
                tokens.push(LispToken {
                    kind: LispTokenKind::String(parsed),
                    start,
                    end,
                    leading_trivia,
                });
                while index < chars.len() && chars[index].0 < end {
                    index += 1;
                }
            }
            'b' if chars.get(index + 1).is_some_and(|(_, next)| *next == '"') => {
                let end = scan_lisp_byte_string_end(source, &chars, index)?;
                let raw = &source[start..end];
                let parsed = super::forms::parse_byte_string_literal(raw)?;
                budget.check_blob_bytes(codec, parsed.len())?;
                tokens.push(LispToken {
                    kind: LispTokenKind::Bytes(parsed),
                    start,
                    end,
                    leading_trivia,
                });
                while index < chars.len() && chars[index].0 < end {
                    index += 1;
                }
            }
            _ => {
                let end = scan_lisp_atom_end(source, &chars, index);
                tokens.push(LispToken {
                    kind: LispTokenKind::Atom(source[start..end].to_owned()),
                    start,
                    end,
                    leading_trivia,
                });
                while index < chars.len() && chars[index].0 < end {
                    index += 1;
                }
            }
        }
    }

    Ok(tokens)
}

fn scan_lisp_string_end(
    source: &str,
    chars: &[(usize, char)],
    start_index: usize,
) -> Result<usize> {
    let mut index = start_index + 1;
    let mut escaped = false;
    while index < chars.len() {
        let (offset, ch) = chars[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
                index += 1;
            }
            '"' => return Ok(offset + ch.len_utf8()),
            _ => index += 1,
        }
    }
    Err(sim_kernel::Error::Eval(format!(
        "unterminated string literal {}",
        &source[chars[start_index].0..]
    )))
}

fn scan_lisp_byte_string_end(
    source: &str,
    chars: &[(usize, char)],
    start_index: usize,
) -> Result<usize> {
    scan_lisp_string_end(source, chars, start_index + 1)
}

fn scan_lisp_atom_end(source: &str, chars: &[(usize, char)], start_index: usize) -> usize {
    let mut index = start_index + 1;
    while index < chars.len() {
        let (offset, ch) = chars[index];
        if ch.is_whitespace() || matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ';' | '\'') {
            return offset;
        }
        index += 1;
    }
    source.len()
}

pub(crate) fn matches_closer(delimiter: Delimiter, token: &LispTokenKind) -> bool {
    matches!(
        (delimiter, token),
        (Delimiter::Parenthesis, LispTokenKind::CloseParen)
            | (Delimiter::Bracket, LispTokenKind::CloseBracket)
            | (Delimiter::Brace, LispTokenKind::CloseBrace)
            | (Delimiter::None, LispTokenKind::CloseBrace)
    )
}

pub(crate) fn extend_tree_trivia(tree: &mut sim_kernel::LocatedExprTree, trivia: Vec<Trivia>) {
    if trivia.is_empty() {
        return;
    }
    if let Some(origin) = &mut tree.origin {
        origin.trivia.extend(trivia);
    }
}

pub(crate) fn strip_lisp_line_comments_preserve_layout(source: &str) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            index += 1;
            continue;
        }

        if ch == ';' {
            out.push(' ');
            index += 1;
            while index < chars.len() && chars[index] != '\n' {
                out.push(' ');
                index += 1;
            }
            continue;
        }

        out.push(ch);
        index += 1;
    }

    out
}

pub(crate) fn origin_from_lisp_source(
    codec: sim_kernel::CodecId,
    source_id: SourceId,
    source: &str,
) -> Origin {
    let (start, leading) = scan_lisp_prefix_trivia(source);
    let (end, trailing) = scan_lisp_suffix_trivia(source, start);
    let mut trivia = leading;
    trivia.extend(trailing);
    Origin {
        codec,
        source: source_id,
        span: Span { start, end },
        trivia,
    }
}

fn scan_lisp_prefix_trivia(source: &str) -> (usize, Vec<Trivia>) {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut trivia = Vec::new();
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            trivia.push(Trivia::Whitespace(source[start..index].to_owned()));
            continue;
        }
        if bytes[index] == b';' {
            let start = index;
            index += 1;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            trivia.push(Trivia::LineComment(source[start..index].to_owned()));
            continue;
        }
        break;
    }
    (index, trivia)
}

fn scan_lisp_suffix_trivia(source: &str, start: usize) -> (usize, Vec<Trivia>) {
    let mut end = source.len();
    let mut trivia = Vec::new();
    while end > start {
        let prefix = &source[..end];
        let bytes = prefix.as_bytes();
        let mut changed = false;

        let mut ws_start = end;
        while ws_start > start && bytes[ws_start - 1].is_ascii_whitespace() {
            ws_start -= 1;
        }
        if ws_start != end {
            trivia.push(Trivia::Whitespace(source[ws_start..end].to_owned()));
            end = ws_start;
            changed = true;
        }

        if end > start {
            let line_start = prefix[..end].rfind('\n').map(|i| i + 1).unwrap_or(0);
            if source[line_start..end].starts_with(';') {
                trivia.push(Trivia::LineComment(source[line_start..end].to_owned()));
                end = line_start;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
    trivia.reverse();
    (end, trivia)
}
