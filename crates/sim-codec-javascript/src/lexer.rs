//! Goal-sensitive bounded ECMAScript tokenizer.

use crate::{Diagnostic, DiagnosticCode as Code, LexicalGoal, Limits, Span, Token, TokenKind};

const KEYWORDS: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const PUNCTUATORS: &[&str] = &[
    ">>>=", "**=", "&&=", "||=", "??=", "===", "!==", ">>>", "<<=", ">>=", "=>", "==", "!=", "<=",
    ">=", "++", "--", "<<", ">>", "&&", "||", "??", "**", "?.", "+=", "-=", "*=", "/=", "%=", "&=",
    "|=", "^=", "...", "{", "}", "(", ")", "[", "]", ".", ";", ",", "<", ">", "+", "-", "*", "%",
    "&", "|", "^", "!", "~", "?", ":", "=",
];

/// Tokenizes with default limits, selecting slash goals from preceding tokens.
pub fn tokenize(source: &str) -> Result<Vec<Token>, Diagnostic> {
    tokenize_with_limits(source, Limits::default())
}
/// Tokenizes with explicit resource limits.
pub fn tokenize_with_limits(source: &str, limits: Limits) -> Result<Vec<Token>, Diagnostic> {
    Lexer::new(source, limits).run()
}

struct Lexer<'a> {
    source: &'a str,
    limits: Limits,
    pos: usize,
    line: usize,
    column: usize,
    out: Vec<Token>,
    goal: LexicalGoal,
    nesting: usize,
}
impl<'a> Lexer<'a> {
    fn new(source: &'a str, limits: Limits) -> Self {
        Self {
            source,
            limits,
            pos: 0,
            line: 1,
            column: 0,
            out: Vec::new(),
            goal: LexicalGoal::RegExp,
            nesting: 0,
        }
    }
    fn run(mut self) -> Result<Vec<Token>, Diagnostic> {
        if self.source.len() > self.limits.max_bytes {
            return Err(self.err(Code::ResourceLimit, 0, "source byte limit exceeded"));
        }
        if self.source.starts_with("#!") {
            let s = self.pos;
            self.take_until_line();
            self.emit(TokenKind::Trivia, s)?;
        }
        while self.pos < self.source.len() {
            if self.line > self.limits.max_lines {
                return Err(self.err(
                    Code::ResourceLimit,
                    self.pos,
                    "physical line limit exceeded",
                ));
            }
            let s = self.pos;
            let ch = self.peek().expect("in source");
            if ch.is_whitespace() {
                self.take_while(char::is_whitespace);
                self.emit(TokenKind::Trivia, s)?;
                continue;
            }
            if self.rest().starts_with("//") {
                self.take_until_line();
                self.emit(TokenKind::Trivia, s)?;
                continue;
            }
            if self.rest().starts_with("/*") {
                self.block_comment(s)?;
                self.emit(TokenKind::Trivia, s)?;
                continue;
            }
            if is_id_start(ch) || (ch == '\\' && self.rest().starts_with("\\u")) {
                self.identifier(s)?;
                continue;
            }
            if ch == '#'
                && self
                    .rest()
                    .get(1..)
                    .is_some_and(|tail| tail.starts_with(is_id_start))
            {
                self.bump();
                self.take_while(is_id_continue);
                self.emit(TokenKind::Identifier, s)?;
                self.goal = LexicalGoal::Div;
                continue;
            }
            if ch.is_ascii_digit()
                || (ch == '.' && self.rest()[1..].starts_with(|c: char| c.is_ascii_digit()))
            {
                self.number(s)?;
                continue;
            }
            if matches!(ch, '\'' | '"') {
                self.string(s, ch)?;
                self.emit(TokenKind::String, s)?;
                self.goal = LexicalGoal::Div;
                continue;
            }
            if ch == '`' {
                self.template(s)?;
                self.emit(TokenKind::Template, s)?;
                self.goal = LexicalGoal::Div;
                continue;
            }
            if ch == '/' && self.goal == LexicalGoal::RegExp {
                self.regexp(s)?;
                self.emit(TokenKind::RegExp, s)?;
                self.goal = LexicalGoal::Div;
                continue;
            }
            if ch == '/' {
                self.bump();
                if self.peek() == Some('=') {
                    self.bump();
                }
                self.emit(TokenKind::Punctuator, s)?;
                self.goal = LexicalGoal::RegExp;
                continue;
            }
            let Some(p) = PUNCTUATORS
                .iter()
                .find(|p| self.rest().starts_with(**p))
                .copied()
            else {
                self.bump();
                return Err(self.err(
                    Code::InvalidCharacter,
                    s,
                    "invalid ECMAScript source character",
                ));
            };
            for _ in p.chars() {
                self.bump();
            }
            if matches!(p, "(" | "[" | "{") {
                self.nesting += 1;
                if self.nesting > self.limits.max_nesting {
                    return Err(self.err(
                        Code::ResourceLimit,
                        s,
                        "delimiter nesting limit exceeded",
                    ));
                }
            }
            if matches!(p, ")" | "]" | "}") {
                self.nesting = self.nesting.saturating_sub(1);
            }
            self.emit(TokenKind::Punctuator, s)?;
            self.goal = if token_ends_expression(p) {
                LexicalGoal::Div
            } else {
                LexicalGoal::RegExp
            };
        }
        self.emit(TokenKind::End, self.pos)?;
        Ok(self.out)
    }
    fn identifier(&mut self, s: usize) -> Result<(), Diagnostic> {
        if self.peek() == Some('\\') {
            self.escape()?;
        } else {
            self.bump();
        }
        while let Some(c) = self.peek() {
            if is_id_continue(c) {
                self.bump();
            } else if c == '\\' && self.rest().starts_with("\\u") {
                self.escape()?;
            } else {
                break;
            }
        }
        let raw = &self.source[s..self.pos];
        let kind = if !raw.contains('\\') && KEYWORDS.contains(&raw) {
            TokenKind::Keyword
        } else {
            TokenKind::Identifier
        };
        self.emit(kind, s)?;
        self.goal = LexicalGoal::Div;
        Ok(())
    }
    fn escape(&mut self) -> Result<(), Diagnostic> {
        let s = self.pos;
        self.bump();
        if self.peek() != Some('u') {
            return Err(self.err(
                Code::InvalidCharacter,
                s,
                "identifier escape must be Unicode",
            ));
        }
        self.bump();
        if self.peek() == Some('{') {
            self.bump();
            let d = self.take_while(|c| c.is_ascii_hexdigit());
            if d == 0 || self.peek() != Some('}') {
                return Err(self.err(Code::InvalidCharacter, s, "invalid Unicode escape"));
            }
            self.bump();
        } else {
            for _ in 0..4 {
                if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    return Err(self.err(Code::InvalidCharacter, s, "invalid Unicode escape"));
                }
                self.bump();
            }
        }
        Ok(())
    }
    fn number(&mut self, s: usize) -> Result<(), Diagnostic> {
        if self.rest().starts_with("0x") || self.rest().starts_with("0X") {
            self.bump();
            self.bump();
            self.digits(s, 16)?;
        } else if self.rest().starts_with("0b") || self.rest().starts_with("0B") {
            self.bump();
            self.bump();
            self.digits(s, 2)?;
        } else if self.rest().starts_with("0o") || self.rest().starts_with("0O") {
            self.bump();
            self.bump();
            self.digits(s, 8)?;
        } else {
            self.take_while(|c| c.is_ascii_digit() || c == '_');
            if self.peek() == Some('.') {
                self.bump();
                self.take_while(|c| c.is_ascii_digit() || c == '_');
            }
            if self.peek().is_some_and(|c| matches!(c, 'e' | 'E')) {
                self.bump();
                if self.peek().is_some_and(|c| matches!(c, '+' | '-')) {
                    self.bump();
                }
                let n = self.take_while(|c| c.is_ascii_digit() || c == '_');
                if n == 0 {
                    return Err(self.err(Code::InvalidCharacter, s, "invalid numeric exponent"));
                }
            }
        }
        if self.peek() == Some('n') {
            self.bump();
        }
        self.emit(TokenKind::Number, s)?;
        self.goal = LexicalGoal::Div;
        Ok(())
    }
    fn digits(&mut self, s: usize, radix: u32) -> Result<(), Diagnostic> {
        let n = self.take_while(|c| c == '_' || c.is_digit(radix));
        if n == 0 {
            return Err(self.err(Code::InvalidCharacter, s, "radix literal requires digits"));
        }
        Ok(())
    }
    fn string(&mut self, s: usize, q: char) -> Result<(), Diagnostic> {
        self.bump();
        loop {
            match self.peek() {
                None | Some('\n' | '\r') => {
                    return Err(self.err(
                        Code::UnterminatedLiteral,
                        s,
                        "unterminated string literal",
                    ));
                }
                Some(c) if c == q => {
                    self.bump();
                    return Ok(());
                }
                Some('\\') => {
                    self.bump();
                    if self.peek().is_some() {
                        self.bump();
                    }
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
    }
    fn template(&mut self, s: usize) -> Result<(), Diagnostic> {
        self.bump();
        let mut fields = 0usize;
        loop {
            match self.peek() {
                None => {
                    return Err(self.err(
                        Code::UnterminatedLiteral,
                        s,
                        "unterminated template literal",
                    ));
                }
                Some('\\') => {
                    self.bump();
                    if self.peek().is_some() {
                        self.bump();
                    }
                }
                Some('`') if fields == 0 => {
                    self.bump();
                    return Ok(());
                }
                Some('$') if self.rest().starts_with("${") => {
                    self.bump();
                    self.bump();
                    fields += 1;
                    if fields > self.limits.max_nesting {
                        return Err(self.err(
                            Code::ResourceLimit,
                            s,
                            "template nesting limit exceeded",
                        ));
                    }
                }
                Some('}') if fields > 0 => {
                    self.bump();
                    fields -= 1;
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
    }
    fn regexp(&mut self, s: usize) -> Result<(), Diagnostic> {
        self.bump();
        let mut class = false;
        let mut body = false;
        loop {
            match self.peek() {
                None | Some('\n' | '\r') => {
                    return Err(self.err(
                        Code::UnterminatedLiteral,
                        s,
                        "unterminated regular-expression literal",
                    ));
                }
                Some('\\') => {
                    body = true;
                    self.bump();
                    if self.peek().is_some() {
                        self.bump();
                    }
                }
                Some('[') => {
                    body = true;
                    class = true;
                    self.bump();
                }
                Some(']') => {
                    class = false;
                    self.bump();
                }
                Some('/') if !class => {
                    if !body {
                        return Err(self.err(
                            Code::InvalidCharacter,
                            s,
                            "empty regular-expression body",
                        ));
                    }
                    self.bump();
                    self.take_while(is_id_continue);
                    return Ok(());
                }
                Some('*') if !body => {
                    return Err(self.err(
                        Code::InvalidCharacter,
                        s,
                        "regular-expression body cannot begin with '*'",
                    ));
                }
                Some(_) => {
                    body = true;
                    self.bump();
                }
            }
        }
    }
    fn block_comment(&mut self, s: usize) -> Result<(), Diagnostic> {
        self.bump();
        self.bump();
        while self.pos < self.source.len() {
            if self.rest().starts_with("*/") {
                self.bump();
                self.bump();
                return Ok(());
            }
            self.bump();
        }
        Err(self.err(Code::UnterminatedLiteral, s, "unterminated block comment"))
    }
    fn take_until_line(&mut self) {
        self.take_while(|c| !matches!(c, '\n' | '\r'));
    }
    fn emit(&mut self, kind: TokenKind, s: usize) -> Result<(), Diagnostic> {
        if self.out.len() >= self.limits.max_tokens {
            return Err(self.err(Code::ResourceLimit, s, "token limit exceeded"));
        }
        let (line, column) = location(self.source, s);
        self.out.push(Token {
            kind,
            span: Span {
                start: s,
                end: self.pos,
            },
            line,
            column,
            goal: self.goal,
        });
        Ok(())
    }
    fn err(&self, code: Code, s: usize, message: &str) -> Diagnostic {
        let (line, column) = location(self.source, s);
        Diagnostic {
            code,
            span: Span {
                start: s,
                end: self.pos.max(s),
            },
            line,
            column,
            message: message.to_owned(),
        }
    }
    fn rest(&self) -> &str {
        &self.source[self.pos..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 0
        } else {
            self.column += 1
        }
        Some(c)
    }
    fn take_while(&mut self, p: impl Fn(char) -> bool) -> usize {
        let mut n = 0;
        while self.peek().is_some_and(&p) {
            self.bump();
            n += 1;
        }
        n
    }
}
fn is_id_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}
fn is_id_continue(c: char) -> bool {
    is_id_start(c) || c.is_alphanumeric() || matches!(c, '\u{200c}' | '\u{200d}')
}
fn token_ends_expression(p: &str) -> bool {
    matches!(p, ")" | "]" | "}" | "++" | "--")
}
fn location(source: &str, pos: usize) -> (usize, usize) {
    let before = &source[..pos];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let col = before
        .rsplit_once('\n')
        .map_or(before, |(_, x)| x)
        .chars()
        .count();
    (line, col)
}
