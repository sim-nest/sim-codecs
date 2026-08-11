//! Bounded Python 3.14 tokenizer with explicit layout and trivia.

use crate::{Diagnostic, DiagnosticCode as Code, Limits, Span, Token, TokenKind};

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];
const OPS: &[&str] = &[
    "**=", ">>=", "<<=", "//=", "...", "->", ":=", "==", "!=", "<=", ">=", "<<", ">>", "**", "//",
    "+=", "-=", "*=", "/=", "%=", "@=", "&=", "|=", "^=", "=>", "+", "-", "*", "/", "%", "@", "&",
    "|", "^", "~", ":", ",", ";", ".", "=", "(", ")", "[", "]", "{", "}", "<", ">",
];

/// Tokenize with default resource bounds.
pub fn tokenize(source: &str) -> Result<Vec<Token>, Diagnostic> {
    tokenize_with_limits(source, Limits::default())
}

/// Tokenize with explicit resource bounds.
pub fn tokenize_with_limits(source: &str, limits: Limits) -> Result<Vec<Token>, Diagnostic> {
    Lexer::new(source, limits).run()
}

struct Lexer<'a> {
    source: &'a str,
    limits: Limits,
    pos: usize,
    line: usize,
    column: usize,
    at_line_start: bool,
    bracket_depth: usize,
    indents: Vec<(usize, String)>,
    out: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, limits: Limits) -> Self {
        Self {
            source,
            limits,
            pos: 0,
            line: 1,
            column: 0,
            at_line_start: true,
            bracket_depth: 0,
            indents: vec![(0, String::new())],
            out: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Vec<Token>, Diagnostic> {
        if self.source.len() > self.limits.max_bytes {
            return Err(self.error(Code::ResourceLimit, 0, "source byte limit exceeded"));
        }
        while self.pos < self.source.len() {
            if self.line > self.limits.max_lines {
                return Err(self.error(
                    Code::ResourceLimit,
                    self.pos,
                    "physical line limit exceeded",
                ));
            }
            if self.at_line_start && self.bracket_depth == 0 {
                self.layout()?;
            }
            if self.pos >= self.source.len() {
                break;
            }
            let start = self.pos;
            let ch = self.peek().expect("position is in source");
            if ch == '\n' || ch == '\r' {
                self.newline()?;
                continue;
            }
            if matches!(ch, ' ' | '\t' | '\u{0c}') {
                self.take_while(|c| matches!(c, ' ' | '\t' | '\u{0c}'));
                self.emit(TokenKind::Trivia, start)?;
                continue;
            }
            if ch == '#' {
                self.take_while(|c| !matches!(c, '\r' | '\n'));
                self.emit(TokenKind::Trivia, start)?;
                continue;
            }
            if ch == '\\'
                && self
                    .rest()
                    .get(1..)
                    .is_some_and(|s| s.starts_with('\n') || s.starts_with("\r\n"))
            {
                self.bump();
                if self.peek() == Some('\r') {
                    self.bump();
                }
                self.bump();
                self.emit(TokenKind::Trivia, start)?;
                continue;
            }
            if is_name_start(ch) {
                self.name_or_string(start)?;
                continue;
            }
            if ch.is_ascii_digit()
                || (ch == '.'
                    && self
                        .rest()
                        .get(1..)
                        .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit())))
            {
                self.number(start)?;
                continue;
            }
            if matches!(ch, '\'' | '"') {
                self.string(start, "", TokenKind::String)?;
                continue;
            }
            if let Some(op) = OPS.iter().find(|op| self.rest().starts_with(**op)) {
                let op = *op;
                for _ in op.chars() {
                    self.bump();
                }
                match op {
                    "(" | "[" | "{" => {
                        self.bracket_depth += 1;
                        if self.bracket_depth > self.limits.max_nesting {
                            return Err(self.error(
                                Code::ResourceLimit,
                                start,
                                "delimiter nesting limit exceeded",
                            ));
                        }
                    }
                    ")" | "]" | "}" => self.bracket_depth = self.bracket_depth.saturating_sub(1),
                    _ => {}
                }
                self.emit(TokenKind::Operator, start)?;
                continue;
            }
            self.bump();
            return Err(self.error(
                Code::InvalidCharacter,
                start,
                "invalid Python source character",
            ));
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            self.emit_zero(TokenKind::Dedent)?;
        }
        self.emit_zero(TokenKind::End)?;
        Ok(self.out)
    }

    fn layout(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        while matches!(self.peek(), Some(' ' | '\t' | '\u{0c}')) {
            self.bump();
        }
        let prefix = &self.source[start..self.pos];
        if self.peek().is_none() || matches!(self.peek(), Some('\r' | '\n' | '#')) {
            if self.pos > start {
                self.emit(TokenKind::Trivia, start)?;
            }
            self.at_line_start = false;
            return Ok(());
        }
        let width = indent_width(prefix);
        let (current, current_prefix) = self.indents.last().expect("base indent").clone();
        if width == current
            && prefix != current_prefix
            && prefix.contains('\t') != current_prefix.contains('\t')
        {
            return Err(self.error(
                Code::AmbiguousIndentation,
                start,
                "inconsistent tabs and spaces in indentation",
            ));
        }
        if self.pos > start {
            self.emit(TokenKind::Trivia, start)?;
        }
        if width > current {
            if self.indents.len() >= self.limits.max_nesting {
                return Err(self.error(
                    Code::ResourceLimit,
                    start,
                    "indentation nesting limit exceeded",
                ));
            }
            self.indents.push((width, prefix.to_owned()));
            self.emit_zero(TokenKind::Indent)?;
        } else if width < current {
            while self.indents.last().is_some_and(|(w, _)| *w > width) {
                self.indents.pop();
                self.emit_zero(TokenKind::Dedent)?;
            }
            if self.indents.last().map(|x| x.0) != Some(width) {
                return Err(self.error(
                    Code::InvalidIndentation,
                    start,
                    "unindent does not match an outer level",
                ));
            }
        }
        self.at_line_start = false;
        Ok(())
    }

    fn name_or_string(&mut self, start: usize) -> Result<(), Diagnostic> {
        self.take_while(is_name_continue);
        let word = &self.source[start..self.pos];
        if self.peek().is_some_and(|c| matches!(c, '\'' | '"')) && is_string_prefix(word) {
            let kind = if word.to_ascii_lowercase().contains('f') {
                TokenKind::FString
            } else if word.to_ascii_lowercase().contains('t') {
                TokenKind::TemplateString
            } else {
                TokenKind::String
            };
            self.string(start, word, kind)
        } else {
            self.emit(
                if KEYWORDS.contains(&word) {
                    TokenKind::Keyword
                } else {
                    TokenKind::Name
                },
                start,
            )
        }
    }

    fn string(&mut self, start: usize, _prefix: &str, kind: TokenKind) -> Result<(), Diagnostic> {
        let quote = self.peek().expect("string quote");
        self.bump();
        let triple = self.rest().starts_with(quote)
            && self
                .rest()
                .get(quote.len_utf8()..)
                .is_some_and(|s| s.starts_with(quote));
        if triple {
            self.bump();
            self.bump();
        }
        let mut braces = 0usize;
        loop {
            let Some(ch) = self.peek() else {
                return Err(self.error(
                    Code::UnterminatedLiteral,
                    start,
                    "unterminated string literal",
                ));
            };
            if ch == '\\' {
                self.bump();
                if self.peek().is_some() {
                    self.bump();
                }
                continue;
            }
            if matches!(kind, TokenKind::FString | TokenKind::TemplateString) {
                if ch == '{' && !self.rest().starts_with("{{") {
                    braces += 1;
                    if braces > self.limits.max_nesting {
                        return Err(self.error(
                            Code::ResourceLimit,
                            self.pos,
                            "interpolation nesting limit exceeded",
                        ));
                    }
                }
                if ch == '}' && !self.rest().starts_with("}}") {
                    if braces == 0 {
                        return Err(self.error(
                            Code::InvalidSyntax,
                            self.pos,
                            "single closing brace in interpolated string",
                        ));
                    }
                    braces -= 1;
                }
            }
            if ch == quote {
                self.bump();
                if !triple
                    || (self.peek() == Some(quote)
                        && self
                            .rest()
                            .get(quote.len_utf8()..)
                            .is_some_and(|s| s.starts_with(quote)))
                {
                    if triple {
                        self.bump();
                        self.bump();
                    }
                    if braces != 0 {
                        return Err(self.error(
                            Code::UnterminatedLiteral,
                            start,
                            "unterminated interpolation field",
                        ));
                    }
                    return self.emit(kind, start);
                }
                continue;
            }
            if !triple && matches!(ch, '\r' | '\n') {
                return Err(self.error(
                    Code::UnterminatedLiteral,
                    start,
                    "unterminated string literal",
                ));
            }
            self.bump();
        }
    }

    fn number(&mut self, start: usize) -> Result<(), Diagnostic> {
        self.take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'));
        if matches!(self.peek(), Some('+' | '-'))
            && self.source[start..self.pos].ends_with(['e', 'E'])
        {
            self.bump();
            self.take_while(|c| c.is_ascii_digit() || c == '_');
        }
        self.emit(TokenKind::Number, start)
    }

    fn newline(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
        self.emit(TokenKind::Newline, start)?;
        self.line += 1;
        self.column = 0;
        self.at_line_start = true;
        Ok(())
    }

    fn emit(&mut self, kind: TokenKind, start: usize) -> Result<(), Diagnostic> {
        let (line, column) = locate(self.source, start);
        self.push(Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
            line,
            column,
        })
    }
    fn emit_zero(&mut self, kind: TokenKind) -> Result<(), Diagnostic> {
        self.push(Token {
            kind,
            span: Span {
                start: self.pos,
                end: self.pos,
            },
            line: self.line,
            column: self.column,
        })
    }
    fn push(&mut self, token: Token) -> Result<(), Diagnostic> {
        if self.out.len() >= self.limits.max_tokens {
            return Err(self.error(
                Code::ResourceLimit,
                token.span.start,
                "token limit exceeded",
            ));
        }
        self.out.push(token);
        Ok(())
    }
    fn rest(&self) -> &str {
        &self.source[self.pos..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
            self.column += 1;
        }
    }
    fn take_while(&mut self, test: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&test) {
            self.bump();
        }
    }
    fn error(&self, code: Code, at: usize, message: &str) -> Diagnostic {
        let (line, column) = locate(self.source, at);
        Diagnostic {
            code,
            span: Span { start: at, end: at },
            line,
            column,
            message: message.to_owned(),
        }
    }
}

fn is_name_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}
fn is_name_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}
fn is_string_prefix(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "r" | "u" | "b" | "br" | "rb" | "f" | "fr" | "rf" | "t" | "tr" | "rt"
    )
}
fn indent_width(s: &str) -> usize {
    s.chars().fold(0, |n, c| match c {
        ' ' => n + 1,
        '\t' => (n / 8 + 1) * 8,
        '\u{0c}' => 0,
        _ => n,
    })
}
fn locate(source: &str, at: usize) -> (usize, usize) {
    let before = &source[..at];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let col = before
        .rsplit_once('\n')
        .map_or(before, |(_, tail)| tail)
        .chars()
        .count();
    (line, col)
}
