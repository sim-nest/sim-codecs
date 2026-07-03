//! Codec-neutral, lossless text form for the data subset of `Expr`.
//!
//! Domain codecs (`codec:scene`, `codec:intent`, ...) must round-trip arbitrary
//! data values without borrowing a general codec's grammar or losing
//! information. This module provides exactly that: a small self-delimiting
//! textual form that round-trips the data subset of `Expr` exactly -- maps,
//! lists, vectors, sets, and the atoms (nil, bool, number, symbol, string,
//! bytes). Eval-only `Expr` forms (calls, infix/prefix/postfix, quotes, blocks,
//! locals, annotations, extensions) are not data and are rejected by
//! [`encode_portable`], which is how a domain codec fails closed.
//!
//! Tag bytes: `_` nil, `T`/`F` bool, `N` number, `S` symbol, `R` string,
//! `B` bytes, `(` list, `[` vector, `{` map, `%(` set. Symbol payloads start
//! with `Q` (qualified) or `U` (unqualified); quoted strings are delimited by
//! `"` with `\\ \" \n \r \t` escapes.

use sim_kernel::{CodecId, Error, Expr, NumberLiteral, Result, Symbol};

/// Serialize a data-subset `Expr` into the codec-neutral portable text form.
///
/// `codec` is used only to tag any error with the calling codec's id.
///
/// # Examples
///
/// ```
/// use sim_codec::{decode_portable, encode_portable};
/// use sim_kernel::{CodecId, Expr};
///
/// let expr = Expr::List(vec![Expr::Nil, Expr::Bool(true), Expr::String("hi".into())]);
/// let text = encode_portable(CodecId(0), &expr).unwrap();
/// assert_eq!(decode_portable(CodecId(0), &text).unwrap(), expr);
///
/// // Eval-only forms are not data and fail closed.
/// assert!(encode_portable(CodecId(0), &Expr::Block(vec![])).is_err());
/// ```
pub fn encode_portable(codec: CodecId, expr: &Expr) -> Result<String> {
    let mut out = String::new();
    write_value(codec, expr, &mut out)?;
    Ok(out)
}

/// Parse codec-neutral portable text back into an `Expr`, failing closed on any
/// malformed input rather than panicking.
pub fn decode_portable(codec: CodecId, source: &str) -> Result<Expr> {
    let mut parser = Parser {
        bytes: source.as_bytes(),
        pos: 0,
        codec,
    };
    let expr = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return Err(parser.error("trailing input after value"));
    }
    Ok(expr)
}

fn unsupported(codec: CodecId, form: &str) -> Error {
    Error::CodecError {
        codec,
        message: format!("portable text cannot encode a non-data expression form: {form}"),
    }
}

fn write_value(codec: CodecId, expr: &Expr, out: &mut String) -> Result<()> {
    match expr {
        Expr::Nil => out.push('_'),
        Expr::Bool(true) => out.push('T'),
        Expr::Bool(false) => out.push('F'),
        Expr::Number(number) => {
            out.push('N');
            write_symbol_payload(&number.domain, out);
            write_qstr(&number.canonical, out);
        }
        Expr::Symbol(symbol) => {
            out.push('S');
            write_symbol_payload(symbol, out);
        }
        Expr::String(text) => {
            out.push('R');
            write_qstr(text, out);
        }
        Expr::Bytes(bytes) => {
            out.push('B');
            write_qstr(&hex_encode(bytes), out);
        }
        Expr::List(items) => write_seq(codec, '(', ')', items, out)?,
        Expr::Vector(items) => write_seq(codec, '[', ']', items, out)?,
        Expr::Set(items) => {
            out.push('%');
            write_seq(codec, '(', ')', items, out)?;
        }
        Expr::Map(entries) => {
            out.push('{');
            for (key, value) in entries {
                out.push(' ');
                write_value(codec, key, out)?;
                out.push(' ');
                write_value(codec, value, out)?;
            }
            out.push_str(" }");
        }
        Expr::Local(_) => return Err(unsupported(codec, "local")),
        Expr::Call { .. } => return Err(unsupported(codec, "call")),
        Expr::Infix { .. } => return Err(unsupported(codec, "infix")),
        Expr::Prefix { .. } => return Err(unsupported(codec, "prefix")),
        Expr::Postfix { .. } => return Err(unsupported(codec, "postfix")),
        Expr::Block(_) => return Err(unsupported(codec, "block")),
        Expr::Quote { .. } => return Err(unsupported(codec, "quote")),
        Expr::Annotated { .. } => return Err(unsupported(codec, "annotated")),
        Expr::Extension { .. } => return Err(unsupported(codec, "extension")),
    }
    Ok(())
}

fn write_seq(
    codec: CodecId,
    open: char,
    close: char,
    items: &[Expr],
    out: &mut String,
) -> Result<()> {
    out.push(open);
    for item in items {
        out.push(' ');
        write_value(codec, item, out)?;
    }
    out.push(' ');
    out.push(close);
    Ok(())
}

fn write_symbol_payload(symbol: &Symbol, out: &mut String) {
    match &symbol.namespace {
        Some(namespace) => {
            out.push('Q');
            write_qstr(namespace, out);
            write_qstr(&symbol.name, out);
        }
        None => {
            out.push('U');
            write_qstr(&symbol.name, out);
        }
    }
}

fn write_qstr(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    codec: CodecId,
}

impl Parser<'_> {
    fn error(&self, message: impl Into<String>) -> Error {
        Error::CodecError {
            codec: self.codec,
            message: format!(
                "portable text decode error at byte {}: {}",
                self.pos,
                message.into()
            ),
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn expect(&mut self, byte: u8) -> Result<()> {
        if self.bump() == Some(byte) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{}'", byte as char)))
        }
    }

    fn parse_value(&mut self) -> Result<Expr> {
        self.skip_ws();
        match self.peek() {
            Some(b'_') => {
                self.pos += 1;
                Ok(Expr::Nil)
            }
            Some(b'T') => {
                self.pos += 1;
                Ok(Expr::Bool(true))
            }
            Some(b'F') => {
                self.pos += 1;
                Ok(Expr::Bool(false))
            }
            Some(b'N') => {
                self.pos += 1;
                let domain = self.parse_symbol_payload()?;
                let canonical = self.parse_qstr()?;
                Ok(Expr::Number(NumberLiteral { domain, canonical }))
            }
            Some(b'S') => {
                self.pos += 1;
                Ok(Expr::Symbol(self.parse_symbol_payload()?))
            }
            Some(b'R') => {
                self.pos += 1;
                Ok(Expr::String(self.parse_qstr()?))
            }
            Some(b'B') => {
                self.pos += 1;
                let hex = self.parse_qstr()?;
                Ok(Expr::Bytes(self.parse_hex(&hex)?))
            }
            Some(b'(') => Ok(Expr::List(self.parse_seq(b'(', b')')?)),
            Some(b'[') => Ok(Expr::Vector(self.parse_seq(b'[', b']')?)),
            Some(b'%') => {
                self.pos += 1;
                Ok(Expr::Set(self.parse_seq(b'(', b')')?))
            }
            Some(b'{') => self.parse_map(),
            Some(other) => Err(self.error(format!("unexpected tag byte '{}'", other as char))),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn parse_seq(&mut self, open: u8, close: u8) -> Result<Vec<Expr>> {
        self.expect(open)?;
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(byte) if byte == close => {
                    self.pos += 1;
                    return Ok(items);
                }
                None => return Err(self.error("unterminated sequence")),
                _ => items.push(self.parse_value()?),
            }
        }
    }

    fn parse_map(&mut self) -> Result<Expr> {
        self.expect(b'{')?;
        let mut entries = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Expr::Map(entries));
                }
                None => return Err(self.error("unterminated map")),
                _ => {
                    let key = self.parse_value()?;
                    let value = self.parse_value()?;
                    entries.push((key, value));
                }
            }
        }
    }

    fn parse_symbol_payload(&mut self) -> Result<Symbol> {
        match self.bump() {
            Some(b'Q') => {
                let namespace = self.parse_qstr()?;
                let name = self.parse_qstr()?;
                Ok(Symbol::qualified(namespace, name))
            }
            Some(b'U') => Ok(Symbol::new(self.parse_qstr()?)),
            _ => Err(self.error("expected symbol payload tag 'Q' or 'U'")),
        }
    }

    fn parse_qstr(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut bytes = Vec::new();
        loop {
            match self.bump() {
                Some(b'"') => {
                    return String::from_utf8(bytes)
                        .map_err(|err| self.error(format!("invalid utf-8 in string: {err}")));
                }
                Some(b'\\') => match self.bump() {
                    Some(b'\\') => bytes.push(b'\\'),
                    Some(b'"') => bytes.push(b'"'),
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b't') => bytes.push(b'\t'),
                    _ => return Err(self.error("invalid escape sequence")),
                },
                Some(byte) => bytes.push(byte),
                None => return Err(self.error("unterminated string")),
            }
        }
    }

    fn parse_hex(&self, hex: &str) -> Result<Vec<u8>> {
        if !hex.len().is_multiple_of(2) {
            return Err(self.error("byte literal has odd hex length"));
        }
        let bytes = hex.as_bytes();
        let mut out = Vec::with_capacity(hex.len() / 2);
        let mut index = 0;
        while index < bytes.len() {
            let hi = hex_digit(bytes[index]).ok_or_else(|| self.error("invalid hex digit"))?;
            let lo = hex_digit(bytes[index + 1]).ok_or_else(|| self.error("invalid hex digit"))?;
            out.push((hi << 4) | lo);
            index += 2;
        }
        Ok(out)
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
