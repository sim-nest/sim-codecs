//! Tokenizer for Algol source: scans infix text into spanned tokens carrying
//! span offsets and leading trivia for round-tripping.

use sim_codec::{DecodeBudget, decode_string_literal};
use sim_kernel::{Error, PrattToken as Token, Result, Trivia};

/// A Pratt token paired with its byte span and the trivia (whitespace and
/// comments) that preceded it, so source layout can be reconstructed on encode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpannedToken {
    /// The scanned token.
    pub token: Token,
    /// Byte offset where the token starts in the source.
    pub start: usize,
    /// Byte offset just past the end of the token.
    pub end: usize,
    /// Whitespace and comment trivia immediately preceding the token.
    pub leading_trivia: Vec<Trivia>,
}

/// Tokenizes Algol `source` into spanned tokens under a default decode budget.
pub fn tokenize_algol_spanned(source: &str) -> Result<Vec<SpannedToken>> {
    let mut budget = DecodeBudget::new(sim_codec::DecodeLimits::default());
    tokenize_algol_spanned_with_budget(source, &mut budget)
}

/// Tokenizes Algol `source` into spanned tokens under an explicit `budget`.
///
/// Scans identifiers, numbers, string literals, operator runs, parentheses, and
/// commas, attaching leading whitespace and comment trivia to each token. The
/// budget bounds trivia and string sizes; an unterminated block comment or an
/// unexpected character is reported as an error.
pub fn tokenize_algol_spanned_with_budget(
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Vec<SpannedToken>> {
    let chars = source.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    let mut tokens = Vec::new();
    let source_len = source.len();

    let char_at = |index: usize| chars.get(index).map(|(_, ch)| *ch);
    let byte_at = |index: usize| chars.get(index).map(|(offset, _)| *offset);
    let end_of = |index: usize| byte_at(index).unwrap_or(source_len);

    while index < chars.len() {
        let mut leading_trivia = Vec::new();
        let ch = char_at(index).unwrap();
        if ch.is_whitespace()
            || (ch == '/' && char_at(index + 1).is_some_and(|next| next == '/' || next == '*'))
        {
            while let Some(ch) = char_at(index) {
                if ch.is_whitespace() {
                    let start = byte_at(index).unwrap();
                    index += 1;
                    while index < chars.len() && char_at(index).is_some_and(char::is_whitespace) {
                        index += 1;
                    }
                    budget.add_trivia(sim_kernel::CodecId(0))?;
                    leading_trivia
                        .push(Trivia::Whitespace(source[start..end_of(index)].to_owned()));
                    continue;
                }
                if ch == '/' && char_at(index + 1) == Some('/') {
                    let start = byte_at(index).unwrap();
                    index += 2;
                    while index < chars.len() && char_at(index) != Some('\n') {
                        index += 1;
                    }
                    budget.add_trivia(sim_kernel::CodecId(0))?;
                    leading_trivia
                        .push(Trivia::LineComment(source[start..end_of(index)].to_owned()));
                    continue;
                }
                if ch == '/' && char_at(index + 1) == Some('*') {
                    let start = byte_at(index).unwrap();
                    index += 2;
                    let mut closed = false;
                    while index < chars.len() {
                        if char_at(index) == Some('*') && char_at(index + 1) == Some('/') {
                            index += 2;
                            closed = true;
                            break;
                        }
                        index += 1;
                    }
                    if !closed {
                        return Err(Error::CodecError {
                            codec: sim_kernel::CodecId(0),
                            message: "unterminated algol block comment".to_owned(),
                        });
                    }
                    budget.add_trivia(sim_kernel::CodecId(0))?;
                    leading_trivia.push(Trivia::BlockComment(
                        source[start..end_of(index.min(chars.len()))].to_owned(),
                    ));
                    continue;
                }
                break;
            }
            if index >= chars.len() {
                break;
            }
        }
        let ch = char_at(index).unwrap();
        if ch == '(' {
            let start = byte_at(index).unwrap();
            index += 1;
            tokens.push(SpannedToken {
                token: Token::OpenParen,
                start,
                end: end_of(index),
                leading_trivia,
            });
            continue;
        }
        if ch == ')' {
            let start = byte_at(index).unwrap();
            index += 1;
            tokens.push(SpannedToken {
                token: Token::CloseParen,
                start,
                end: end_of(index),
                leading_trivia,
            });
            continue;
        }
        if ch == ',' {
            let start = byte_at(index).unwrap();
            index += 1;
            tokens.push(SpannedToken {
                token: Token::Comma,
                start,
                end: end_of(index),
                leading_trivia,
            });
            continue;
        }
        if ch == '"' {
            let start = byte_at(index).unwrap();
            index += 1;
            let mut escaped = false;
            while index < chars.len() {
                let next = char_at(index).unwrap();
                if !escaped && next == '"' {
                    index += 1;
                    break;
                }
                escaped = !escaped && next == '\\';
                index += 1;
            }
            let raw = &source[start..end_of(index)];
            let value = decode_string_literal(sim_kernel::CodecId(0), raw)?;
            budget.check_string_bytes(sim_kernel::CodecId(0), value.len())?;
            tokens.push(SpannedToken {
                token: Token::String(value),
                start,
                end: end_of(index),
                leading_trivia,
            });
            continue;
        }
        if ch.is_ascii_digit()
            || (ch == '.' && char_at(index + 1).is_some_and(|c| c.is_ascii_digit()))
        {
            let start = index;
            index += 1;
            while index < chars.len()
                && char_at(index).is_some_and(|c| c.is_ascii_digit() || c == '.')
            {
                index += 1;
            }
            budget.check_string_bytes(
                sim_kernel::CodecId(0),
                source[byte_at(start).unwrap()..end_of(index)].len(),
            )?;
            tokens.push(SpannedToken {
                token: Token::Number(source[byte_at(start).unwrap()..end_of(index)].to_owned()),
                start: byte_at(start).unwrap(),
                end: end_of(index),
                leading_trivia,
            });
            continue;
        }
        if is_ident_start(ch) {
            let start = index;
            index += 1;
            while index < chars.len() && char_at(index).is_some_and(is_ident_continue) {
                index += 1;
            }
            tokens.push(SpannedToken {
                token: Token::Ident(source[byte_at(start).unwrap()..end_of(index)].to_owned()),
                start: byte_at(start).unwrap(),
                end: end_of(index),
                leading_trivia,
            });
            continue;
        }
        if is_operator_char(ch) {
            let start = index;
            index += 1;
            while index < chars.len() && char_at(index).is_some_and(is_operator_char) {
                index += 1;
            }
            tokens.push(SpannedToken {
                token: Token::Operator(source[byte_at(start).unwrap()..end_of(index)].to_owned()),
                start: byte_at(start).unwrap(),
                end: end_of(index),
                leading_trivia,
            });
            continue;
        }
        return Err(Error::Eval(format!("unexpected algol character {}", ch)));
    }

    Ok(tokens)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '/' | '?')
}

fn is_operator_char(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '^' | '!' | '%' | '=' | '<' | '>' | '&' | '|' | '~'
    )
}
