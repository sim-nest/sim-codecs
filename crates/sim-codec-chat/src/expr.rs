//! The `SIMCHAT1` text grammar: encodes an `Expr` transcript to canonical chat
//! text and parses that text back to an `Expr`, under a decode budget.

use sim_codec::DecodeBudget;
use sim_kernel::{CodecId, Error, Expr, NumberLiteral, QuoteMode, Result, Symbol};

use crate::base64::{base64_decode, base64_encode};

const HEADER: &str = "SIMCHAT1\n";

/// Cap on the initial capacity reserved for an attacker-declared collection
/// length. The length is checked against the collection-length limit, but eager
/// `Vec::with_capacity(len)` at every nesting level lets a small, deeply nested
/// input force gigabytes of simultaneous reservations. We reserve at most this
/// many slots up front and let the vector grow as items actually decode; the
/// node-count and depth budgets bound the realized total.
const ALLOC_RESERVE_CAP: usize = 4096;

pub(crate) fn encode_chat_text(expr: &Expr) -> String {
    let mut out = String::from(HEADER);
    encode_node(expr, &mut out);
    out
}

pub(crate) fn decode_chat_text(
    codec: CodecId,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let body = source
        .strip_prefix(HEADER)
        .ok_or_else(|| codec_error(codec, "chat transcript must start with SIMCHAT1 header"))?;
    let mut parser = Parser {
        codec,
        bytes: body.as_bytes(),
        index: 0,
        budget,
    };
    let expr = parser.parse_node(0)?;
    if parser.index != parser.bytes.len() {
        return Err(codec_error(codec, "trailing data after chat transcript"));
    }
    Ok(expr)
}

fn encode_node(expr: &Expr, out: &mut String) {
    match expr {
        Expr::Nil => out.push_str("z;"),
        Expr::Bool(true) => out.push_str("t;"),
        Expr::Bool(false) => out.push_str("f;"),
        Expr::Number(number) => {
            out.push('d');
            encode_text(&number.domain.to_string(), out);
            encode_text(&number.canonical, out);
            out.push(';');
        }
        Expr::Symbol(symbol) => {
            out.push('y');
            encode_text(&symbol.to_string(), out);
            out.push(';');
        }
        Expr::Local(symbol) => {
            out.push('l');
            encode_text(&symbol.to_string(), out);
            out.push(';');
        }
        Expr::String(text) => {
            out.push('s');
            encode_text(text, out);
            out.push(';');
        }
        Expr::Bytes(bytes) => {
            out.push('x');
            encode_text(&base64_encode(bytes), out);
            out.push(';');
        }
        Expr::List(items) => encode_sequence('a', items, out),
        Expr::Vector(items) => encode_sequence('v', items, out),
        Expr::Map(entries) => {
            let mut sorted = entries.clone();
            sorted.sort_by_key(|(key, value)| (key.canonical_key(), value.canonical_key()));
            out.push('m');
            encode_count(sorted.len(), out);
            for (key, value) in sorted {
                encode_node(&key, out);
                encode_node(&value, out);
            }
            out.push(';');
        }
        Expr::Set(items) => {
            let mut sorted = items.clone();
            sorted.sort_by_key(Expr::canonical_key);
            encode_sequence('e', &sorted, out);
        }
        Expr::Call { operator, args } => {
            out.push('c');
            encode_count(args.len(), out);
            encode_node(operator, out);
            for arg in args {
                encode_node(arg, out);
            }
            out.push(';');
        }
        Expr::Infix {
            operator,
            left,
            right,
        } => {
            out.push('i');
            encode_text(&operator.to_string(), out);
            encode_node(left, out);
            encode_node(right, out);
            out.push(';');
        }
        Expr::Prefix { operator, arg } => {
            out.push('p');
            encode_text(&operator.to_string(), out);
            encode_node(arg, out);
            out.push(';');
        }
        Expr::Postfix { operator, arg } => {
            out.push('o');
            encode_text(&operator.to_string(), out);
            encode_node(arg, out);
            out.push(';');
        }
        Expr::Block(items) => encode_sequence('k', items, out),
        Expr::Quote { mode, expr } => {
            out.push('q');
            out.push(quote_mode_code(*mode));
            out.push(':');
            encode_node(expr, out);
            out.push(';');
        }
        Expr::Annotated { expr, annotations } => {
            out.push('r');
            encode_count(annotations.len(), out);
            encode_node(expr, out);
            for (symbol, value) in annotations {
                encode_text(&symbol.to_string(), out);
                encode_node(value, out);
            }
            out.push(';');
        }
        Expr::Extension { tag, payload } => {
            out.push('g');
            encode_text(&tag.to_string(), out);
            encode_node(payload, out);
            out.push(';');
        }
    }
}

fn encode_sequence(tag: char, items: &[Expr], out: &mut String) {
    out.push(tag);
    encode_count(items.len(), out);
    for item in items {
        encode_node(item, out);
    }
    out.push(';');
}

fn encode_count(count: usize, out: &mut String) {
    out.push_str(&count.to_string());
    out.push(':');
}

fn encode_text(text: &str, out: &mut String) {
    out.push_str(&text.len().to_string());
    out.push(':');
    out.push_str(text);
}

struct Parser<'a, 'b> {
    codec: CodecId,
    bytes: &'a [u8],
    index: usize,
    budget: &'b mut DecodeBudget,
}

impl Parser<'_, '_> {
    fn parse_node(&mut self, depth: usize) -> Result<Expr> {
        self.budget.enter_node(self.codec, depth)?;
        let tag = self.next_byte("expected expression tag")?;
        match tag {
            b'z' => {
                self.expect(b';')?;
                Ok(Expr::Nil)
            }
            b't' => {
                self.expect(b';')?;
                Ok(Expr::Bool(true))
            }
            b'f' => {
                self.expect(b';')?;
                Ok(Expr::Bool(false))
            }
            b'd' => {
                let domain = self.parse_text()?;
                let canonical = self.parse_text()?;
                self.expect(b';')?;
                Ok(Expr::Number(NumberLiteral {
                    domain: parse_symbol(&domain),
                    canonical,
                }))
            }
            b'y' => {
                let text = self.parse_text()?;
                self.expect(b';')?;
                Ok(Expr::Symbol(parse_symbol(&text)))
            }
            b'l' => {
                let text = self.parse_text()?;
                self.expect(b';')?;
                Ok(Expr::Local(parse_symbol(&text)))
            }
            b's' => {
                let text = self.parse_text()?;
                self.expect(b';')?;
                Ok(Expr::String(text))
            }
            b'x' => {
                let text = self.parse_text()?;
                self.expect(b';')?;
                Ok(Expr::Bytes(base64_decode(self.codec, &text)?))
            }
            b'a' => self.parse_sequence(depth, Expr::List),
            b'v' => self.parse_sequence(depth, Expr::Vector),
            b'm' => self.parse_map(depth),
            b'e' => self.parse_sequence(depth, Expr::Set),
            b'c' => self.parse_call(depth),
            b'i' => self.parse_infix(depth),
            b'p' => self.parse_prefix(depth),
            b'o' => self.parse_postfix(depth),
            b'k' => self.parse_sequence(depth, Expr::Block),
            b'q' => self.parse_quote(depth),
            b'r' => self.parse_annotated(depth),
            b'g' => self.parse_extension(depth),
            other => Err(codec_error(
                self.codec,
                format!("unknown chat expression tag {}", other as char),
            )),
        }
    }

    fn parse_sequence(
        &mut self,
        depth: usize,
        make: impl FnOnce(Vec<Expr>) -> Expr,
    ) -> Result<Expr> {
        let count = self.parse_len()?;
        self.budget.check_collection_len(self.codec, count)?;
        let mut items = Vec::with_capacity(count.min(ALLOC_RESERVE_CAP));
        for _ in 0..count {
            items.push(self.parse_node(depth + 1)?);
        }
        self.expect(b';')?;
        Ok(make(items))
    }

    fn parse_map(&mut self, depth: usize) -> Result<Expr> {
        let count = self.parse_len()?;
        self.budget.check_collection_len(self.codec, count)?;
        let mut entries = Vec::with_capacity(count.min(ALLOC_RESERVE_CAP));
        for _ in 0..count {
            let key = self.parse_node(depth + 1)?;
            let value = self.parse_node(depth + 1)?;
            entries.push((key, value));
        }
        self.expect(b';')?;
        Ok(Expr::Map(entries))
    }

    fn parse_call(&mut self, depth: usize) -> Result<Expr> {
        let count = self.parse_len()?;
        self.budget.check_collection_len(self.codec, count)?;
        let operator = Box::new(self.parse_node(depth + 1)?);
        let mut args = Vec::with_capacity(count.min(ALLOC_RESERVE_CAP));
        for _ in 0..count {
            args.push(self.parse_node(depth + 1)?);
        }
        self.expect(b';')?;
        Ok(Expr::Call { operator, args })
    }

    fn parse_infix(&mut self, depth: usize) -> Result<Expr> {
        let operator = parse_symbol(&self.parse_text()?);
        let left = Box::new(self.parse_node(depth + 1)?);
        let right = Box::new(self.parse_node(depth + 1)?);
        self.expect(b';')?;
        Ok(Expr::Infix {
            operator,
            left,
            right,
        })
    }

    fn parse_prefix(&mut self, depth: usize) -> Result<Expr> {
        let operator = parse_symbol(&self.parse_text()?);
        let arg = Box::new(self.parse_node(depth + 1)?);
        self.expect(b';')?;
        Ok(Expr::Prefix { operator, arg })
    }

    fn parse_postfix(&mut self, depth: usize) -> Result<Expr> {
        let operator = parse_symbol(&self.parse_text()?);
        let arg = Box::new(self.parse_node(depth + 1)?);
        self.expect(b';')?;
        Ok(Expr::Postfix { operator, arg })
    }

    fn parse_quote(&mut self, depth: usize) -> Result<Expr> {
        let mode = self.next_byte("expected quote mode")?;
        self.expect(b':')?;
        let expr = Box::new(self.parse_node(depth + 1)?);
        self.expect(b';')?;
        Ok(Expr::Quote {
            mode: parse_quote_mode(self.codec, mode)?,
            expr,
        })
    }

    fn parse_annotated(&mut self, depth: usize) -> Result<Expr> {
        let count = self.parse_len()?;
        self.budget.check_collection_len(self.codec, count)?;
        let expr = Box::new(self.parse_node(depth + 1)?);
        let mut annotations = Vec::with_capacity(count.min(ALLOC_RESERVE_CAP));
        for _ in 0..count {
            let symbol = parse_symbol(&self.parse_text()?);
            let value = self.parse_node(depth + 1)?;
            annotations.push((symbol, value));
        }
        self.expect(b';')?;
        Ok(Expr::Annotated { expr, annotations })
    }

    fn parse_extension(&mut self, depth: usize) -> Result<Expr> {
        let tag = parse_symbol(&self.parse_text()?);
        let payload = Box::new(self.parse_node(depth + 1)?);
        self.expect(b';')?;
        Ok(Expr::Extension { tag, payload })
    }

    fn parse_text(&mut self) -> Result<String> {
        let len = self.parse_len()?;
        self.budget.check_string_bytes(self.codec, len)?;
        let end = self
            .index
            .checked_add(len)
            .ok_or_else(|| codec_error(self.codec, "chat text length overflowed input position"))?;
        let Some(raw) = self.bytes.get(self.index..end) else {
            return Err(codec_error(
                self.codec,
                "chat text field exceeds input length",
            ));
        };
        self.index = end;
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|err| codec_error(self.codec, format!("chat text is not valid UTF-8: {err}")))
    }

    fn parse_len(&mut self) -> Result<usize> {
        let start = self.index;
        while matches!(self.bytes.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == start {
            return Err(codec_error(self.codec, "expected decimal length"));
        }
        self.expect(b':')?;
        let text = std::str::from_utf8(&self.bytes[start..self.index - 1])
            .map_err(|err| codec_error(self.codec, format!("invalid length text: {err}")))?;
        text.parse::<usize>()
            .map_err(|err| codec_error(self.codec, format!("invalid decimal length: {err}")))
    }

    fn next_byte(&mut self, message: &'static str) -> Result<u8> {
        let byte = self
            .bytes
            .get(self.index)
            .copied()
            .ok_or_else(|| codec_error(self.codec, message))?;
        self.index += 1;
        Ok(byte)
    }

    fn expect(&mut self, expected: u8) -> Result<()> {
        let found = self.next_byte("unexpected end of chat transcript")?;
        if found != expected {
            return Err(codec_error(
                self.codec,
                format!(
                    "expected byte {:?}, found {:?}",
                    expected as char, found as char
                ),
            ));
        }
        Ok(())
    }
}

fn parse_symbol(raw: &str) -> Symbol {
    match raw.split_once('/') {
        Some((namespace, name)) => Symbol::qualified(namespace.to_owned(), name.to_owned()),
        None => Symbol::new(raw.to_owned()),
    }
}

fn quote_mode_code(mode: QuoteMode) -> char {
    match mode {
        QuoteMode::Quote => '0',
        QuoteMode::QuasiQuote => '1',
        QuoteMode::Unquote => '2',
        QuoteMode::Splice => '3',
        QuoteMode::Syntax => '4',
    }
}

fn parse_quote_mode(codec: CodecId, raw: u8) -> Result<QuoteMode> {
    match raw {
        b'0' => Ok(QuoteMode::Quote),
        b'1' => Ok(QuoteMode::QuasiQuote),
        b'2' => Ok(QuoteMode::Unquote),
        b'3' => Ok(QuoteMode::Splice),
        b'4' => Ok(QuoteMode::Syntax),
        other => Err(codec_error(
            codec,
            format!("unknown quote mode code {}", other as char),
        )),
    }
}

fn codec_error(codec: CodecId, message: impl Into<String>) -> Error {
    Error::CodecError {
        codec,
        message: message.into(),
    }
}
