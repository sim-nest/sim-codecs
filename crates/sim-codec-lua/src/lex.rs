use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{CodecId, Error, Result, Trivia};

use crate::LUA_CODEC_ID;

/// Lua lexical token with byte span and retained leading trivia.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaToken {
    /// The token kind.
    pub kind: LuaTokenKind,
    /// Byte offset where the token starts.
    pub start: usize,
    /// Byte offset just after the token.
    pub end: usize,
    /// Whitespace and comments preceding this token.
    pub leading_trivia: Vec<Trivia>,
}

/// Tokens recognized by the Lua expression lexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LuaTokenKind {
    /// Identifier or keyword text.
    Identifier(String),
    /// Number literal text.
    Number(String),
    /// Decoded short or long string text.
    String(String),
    /// The vararg token `...`.
    Vararg,
    /// Operator token.
    Operator(String),
    /// `(`.
    OpenParen,
    /// `)`.
    CloseParen,
    /// `{`.
    OpenBrace,
    /// `}`.
    CloseBrace,
    /// `[`.
    OpenBracket,
    /// `]`.
    CloseBracket,
    /// `,`.
    Comma,
    /// `;`.
    Semi,
    /// `.`.
    Dot,
    /// `:`.
    Colon,
    /// `::`.
    DoubleColon,
    /// `=`.
    Equal,
}

/// Tokenizes Lua source under default decode limits.
pub fn tokenize_lua(source: &str) -> Result<Vec<LuaToken>> {
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    tokenize_lua_with_budget(LUA_CODEC_ID, source, &mut budget)
}

/// Tokenizes Lua source under an explicit decode budget.
pub fn tokenize_lua_with_budget(
    codec: CodecId,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Vec<LuaToken>> {
    budget.check_input_bytes(codec, source.len())?;
    let mut scanner = Scanner::new(source);
    let mut tokens = Vec::new();
    while !scanner.is_empty() {
        let leading_trivia = scanner.scan_trivia(codec, budget)?;
        if scanner.is_empty() {
            break;
        }
        tokens.push(scanner.scan_token(codec, budget, leading_trivia)?);
    }
    budget.check_tokens(codec, tokens.len())?;
    Ok(tokens)
}

struct Scanner<'a> {
    source: &'a str,
    chars: Vec<(usize, char)>,
    index: usize,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.char_indices().collect(),
            index: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.index >= self.chars.len()
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.index + offset).map(|(_, ch)| *ch)
    }

    fn byte_at_index(&self, index: usize) -> usize {
        self.chars
            .get(index)
            .map(|(byte, _)| *byte)
            .unwrap_or(self.source.len())
    }

    fn current_byte(&self) -> usize {
        self.byte_at_index(self.index)
    }

    fn end_byte(&self) -> usize {
        self.byte_at_index(self.index)
    }

    fn rest_starts_with(&self, expected: &str) -> bool {
        self.source[self.current_byte()..].starts_with(expected)
    }

    fn advance(&mut self, count: usize) {
        self.index += count;
    }

    fn scan_trivia(&mut self, codec: CodecId, budget: &mut DecodeBudget) -> Result<Vec<Trivia>> {
        let mut trivia = Vec::new();
        while let Some(ch) = self.char_at(0) {
            if ch.is_whitespace() {
                let start = self.current_byte();
                self.advance(1);
                while self.char_at(0).is_some_and(char::is_whitespace) {
                    self.advance(1);
                }
                budget.add_trivia(codec)?;
                trivia.push(Trivia::Whitespace(
                    self.source[start..self.end_byte()].to_owned(),
                ));
                continue;
            }
            if self.rest_starts_with("--") {
                let start = self.current_byte();
                self.advance(2);
                if let Some(equals) = self.long_bracket_equals('[') {
                    self.advance(equals + 2);
                    self.scan_long_close(codec, equals, "lua block comment")?;
                    budget.add_trivia(codec)?;
                    trivia.push(Trivia::BlockComment(
                        self.source[start..self.end_byte()].to_owned(),
                    ));
                    continue;
                }
                while let Some(next) = self.char_at(0) {
                    if matches!(next, '\n' | '\r') {
                        break;
                    }
                    self.advance(1);
                }
                budget.add_trivia(codec)?;
                trivia.push(Trivia::LineComment(
                    self.source[start..self.end_byte()].to_owned(),
                ));
                continue;
            }
            break;
        }
        Ok(trivia)
    }

    fn scan_token(
        &mut self,
        codec: CodecId,
        budget: &mut DecodeBudget,
        leading_trivia: Vec<Trivia>,
    ) -> Result<LuaToken> {
        let start = self.current_byte();
        let ch = self.char_at(0).expect("scanner checked non-empty");
        let kind = match ch {
            '(' => {
                self.advance(1);
                LuaTokenKind::OpenParen
            }
            ')' => {
                self.advance(1);
                LuaTokenKind::CloseParen
            }
            '{' => {
                self.advance(1);
                LuaTokenKind::OpenBrace
            }
            '}' => {
                self.advance(1);
                LuaTokenKind::CloseBrace
            }
            '[' => {
                if let Some(equals) = self.long_bracket_equals('[') {
                    self.advance(equals + 2);
                    LuaTokenKind::String(self.scan_long_string(codec, budget, equals)?)
                } else {
                    self.advance(1);
                    LuaTokenKind::OpenBracket
                }
            }
            ']' => {
                self.advance(1);
                LuaTokenKind::CloseBracket
            }
            ',' => {
                self.advance(1);
                LuaTokenKind::Comma
            }
            ';' => {
                self.advance(1);
                LuaTokenKind::Semi
            }
            ':' if self.char_at(1) == Some(':') => {
                self.advance(2);
                LuaTokenKind::DoubleColon
            }
            ':' => {
                self.advance(1);
                LuaTokenKind::Colon
            }
            '=' if self.char_at(1) == Some('=') => self.scan_operator(2),
            '=' => {
                self.advance(1);
                LuaTokenKind::Equal
            }
            '.' if self.rest_starts_with("...") => {
                self.advance(3);
                LuaTokenKind::Vararg
            }
            '.' if self.rest_starts_with("..") => self.scan_operator(2),
            '.' if self.char_at(1).is_some_and(|next| next.is_ascii_digit()) => {
                self.scan_number(codec, budget)?
            }
            '.' => {
                self.advance(1);
                LuaTokenKind::Dot
            }
            '\'' | '"' => LuaTokenKind::String(self.scan_short_string(codec, budget, ch)?),
            _ if ch.is_ascii_digit() => self.scan_number(codec, budget)?,
            _ if is_ident_start(ch) => self.scan_identifier(),
            _ if is_operator_start(ch) => self.scan_symbolic_operator(),
            _ => return Err(Error::Eval(format!("unexpected lua character {ch}"))),
        };
        Ok(LuaToken {
            kind,
            start,
            end: self.end_byte(),
            leading_trivia,
        })
    }

    fn scan_identifier(&mut self) -> LuaTokenKind {
        let start = self.current_byte();
        self.advance(1);
        while self.char_at(0).is_some_and(is_ident_continue) {
            self.advance(1);
        }
        LuaTokenKind::Identifier(self.source[start..self.end_byte()].to_owned())
    }

    fn scan_symbolic_operator(&mut self) -> LuaTokenKind {
        for candidate in ["//", "<<", ">>", "<=", ">=", "~=", "=="] {
            if self.rest_starts_with(candidate) {
                return self.scan_operator(candidate.chars().count());
            }
        }
        self.scan_operator(1)
    }

    fn scan_operator(&mut self, len: usize) -> LuaTokenKind {
        let start = self.current_byte();
        self.advance(len);
        LuaTokenKind::Operator(self.source[start..self.end_byte()].to_owned())
    }

    fn scan_number(&mut self, codec: CodecId, budget: &DecodeBudget) -> Result<LuaTokenKind> {
        let start_index = self.index;
        if self.char_at(0) == Some('0') && matches!(self.char_at(1), Some('x' | 'X')) {
            self.advance(2);
            self.take_while(is_hex_digit);
            if self.char_at(0) == Some('.') {
                self.advance(1);
                self.take_while(is_hex_digit);
            }
            if matches!(self.char_at(0), Some('p' | 'P')) {
                self.advance(1);
                if matches!(self.char_at(0), Some('+' | '-')) {
                    self.advance(1);
                }
                self.take_while(|ch| ch.is_ascii_digit());
            }
        } else {
            self.take_while(|ch| ch.is_ascii_digit());
            if self.char_at(0) == Some('.') && self.char_at(1) != Some('.') {
                self.advance(1);
                self.take_while(|ch| ch.is_ascii_digit());
            }
            if matches!(self.char_at(0), Some('e' | 'E')) {
                self.advance(1);
                if matches!(self.char_at(0), Some('+' | '-')) {
                    self.advance(1);
                }
                self.take_while(|ch| ch.is_ascii_digit());
            }
        }
        let raw = self.source[self.byte_at_index(start_index)..self.end_byte()].to_owned();
        budget.check_string_bytes(codec, raw.len())?;
        Ok(LuaTokenKind::Number(raw))
    }

    fn scan_short_string(
        &mut self,
        codec: CodecId,
        budget: &DecodeBudget,
        quote: char,
    ) -> Result<String> {
        self.advance(1);
        let mut value = String::new();
        while let Some(ch) = self.char_at(0) {
            if ch == quote {
                self.advance(1);
                budget.check_string_bytes(codec, value.len())?;
                return Ok(value);
            }
            if matches!(ch, '\n' | '\r') {
                return Err(Error::Eval("unterminated lua short string".to_owned()));
            }
            if ch == '\\' {
                self.advance(1);
                value.push(self.scan_escape()?);
                continue;
            }
            value.push(ch);
            self.advance(1);
        }
        Err(Error::Eval("unterminated lua short string".to_owned()))
    }

    fn scan_escape(&mut self) -> Result<char> {
        let Some(ch) = self.char_at(0) else {
            return Err(Error::Eval("unterminated lua escape".to_owned()));
        };
        self.advance(1);
        Ok(match ch {
            'a' => '\u{7}',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{b}',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            '\n' => '\n',
            '\r' => '\r',
            other => other,
        })
    }

    fn scan_long_string(
        &mut self,
        codec: CodecId,
        budget: &DecodeBudget,
        equals: usize,
    ) -> Result<String> {
        let content_start = self.current_byte();
        let content_end = self.scan_long_close(codec, equals, "lua long string")?;
        let mut value = self.source[content_start..content_end].to_owned();
        if value.starts_with("\r\n") {
            value.drain(..2);
        } else if value.starts_with('\n') || value.starts_with('\r') {
            value.drain(..1);
        }
        budget.check_string_bytes(codec, value.len())?;
        Ok(value)
    }

    fn scan_long_close(&mut self, codec: CodecId, equals: usize, label: &str) -> Result<usize> {
        loop {
            let Some(ch) = self.char_at(0) else {
                return Err(Error::CodecError {
                    codec,
                    message: format!("unterminated {label}"),
                });
            };
            if ch == ']' && self.matches_long_close(equals) {
                let content_end = self.current_byte();
                self.advance(equals + 2);
                return Ok(content_end);
            }
            self.advance(1);
        }
    }

    fn long_bracket_equals(&self, bracket: char) -> Option<usize> {
        if self.char_at(0) != Some(bracket) {
            return None;
        }
        let mut offset = 1;
        while self.char_at(offset) == Some('=') {
            offset += 1;
        }
        (self.char_at(offset) == Some(bracket)).then_some(offset - 1)
    }

    fn matches_long_close(&self, equals: usize) -> bool {
        if self.char_at(0) != Some(']') {
            return false;
        }
        for offset in 0..equals {
            if self.char_at(offset + 1) != Some('=') {
                return false;
            }
        }
        self.char_at(equals + 1) == Some(']')
    }

    fn take_while(&mut self, predicate: impl Fn(char) -> bool) {
        while self.char_at(0).is_some_and(&predicate) {
            self.advance(1);
        }
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_hex_digit(ch: char) -> bool {
    ch.is_ascii_hexdigit()
}

fn is_operator_start(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '%' | '^' | '#' | '&' | '~' | '|' | '<' | '>'
    )
}
