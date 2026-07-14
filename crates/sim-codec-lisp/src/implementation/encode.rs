//! Encoding for the Lisp codec: renders any `Expr` (or located expression tree)
//! back to s-expression text, respecting its output position and quote forms.

use sim_codec::{
    DecodeLimits, Decoder, Encoder, Input, Output, ReadCx, TreeEncoder, encode_string_literal,
};
use sim_kernel::{
    EncodePosition, Expr, LocatedExprTree, NumberLiteral, ObjectEncoding, Origin, QuoteMode,
    ReadConstructEncodePolicy, Result, Symbol, Trivia, Value, WriteCx,
};

use super::decode::LispProcMacroDecoder;
use super::forms::{may_be_number_literal, parse_logic_var, parse_symbol};

/// Lisp encoder that renders expressions back to s-expression text.
///
/// Implements the codec's [`Encoder`] and [`TreeEncoder`] roles: it serializes
/// any [`Expr`] (or a [`LocatedExprTree`], preserving trivia) to Lisp text,
/// honoring quote forms and the requested output position.
pub struct LispProcMacroEncoder;

impl Encoder for LispProcMacroEncoder {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        Ok(Output::Text(encode_lisp_expr(cx, expr)?))
    }
}

impl TreeEncoder for LispProcMacroEncoder {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output> {
        Ok(Output::Text(encode_lisp_tree(cx, expr)?))
    }
}

fn encode_lisp_expr(cx: &mut WriteCx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Nil => Ok("nil".to_owned()),
        Expr::Bool(true) => Ok("true".to_owned()),
        Expr::Bool(false) => Ok("false".to_owned()),
        Expr::Number(number) => encode_number_lisp(cx, number),
        Expr::Symbol(symbol) => encode_symbol_lisp(cx, symbol),
        Expr::Local(symbol) => encode_local_lisp(cx, symbol),
        Expr::String(value) => Ok(encode_string_literal(value)),
        Expr::Bytes(bytes) => Ok(encode_byte_string_literal(bytes)),
        Expr::List(items) => encode_seq(cx, "(", ")", items),
        Expr::Vector(items) => encode_seq(cx, "[", "]", items),
        Expr::Map(entries) => encode_map(cx, entries),
        Expr::Set(items) => encode_escape_named(cx, "expr:set", items),
        Expr::Call { operator, args } => match cx.options.position {
            EncodePosition::Eval => {
                let mut items = Vec::with_capacity(args.len() + 1);
                items.push((**operator).clone());
                items.extend(args.iter().cloned());
                encode_seq(cx, "(", ")", &items)
            }
            _ => {
                let mut items = Vec::with_capacity(args.len() + 1);
                items.push(operator.as_ref().clone());
                items.extend(args.iter().cloned());
                encode_escape_named(cx, "expr:call", &items)
            }
        },
        Expr::Infix {
            operator,
            left,
            right,
        } => encode_escape_named(
            cx,
            "expr:infix",
            &[
                Expr::String(operator.to_string()),
                left.as_ref().clone(),
                right.as_ref().clone(),
            ],
        ),
        Expr::Prefix { operator, arg } => encode_escape_named(
            cx,
            "expr:prefix",
            &[Expr::String(operator.to_string()), arg.as_ref().clone()],
        ),
        Expr::Postfix { operator, arg } => encode_escape_named(
            cx,
            "expr:postfix",
            &[Expr::String(operator.to_string()), arg.as_ref().clone()],
        ),
        Expr::Block(items) => encode_seq(cx, "{", "}", items),
        Expr::Quote { mode, expr } => encode_quote_lisp(cx, *mode, expr),
        Expr::Annotated { expr, annotations } => {
            let mut items = Vec::with_capacity(annotations.len() + 2);
            items.push(Expr::Symbol(Symbol::qualified("expr", "annotated")));
            items.push(expr.as_ref().clone());
            items.extend(annotations.iter().map(|(symbol, value)| {
                Expr::List(vec![Expr::Symbol(symbol.clone()), value.clone()])
            }));
            encode_seq(cx, "(", ")", &items)
        }
        Expr::Extension { tag, payload } => encode_escape_named(
            cx,
            "expr:extension",
            &[Expr::Symbol(tag.clone()), payload.as_ref().clone()],
        ),
    }
}

fn encode_number_lisp(cx: &mut WriteCx<'_>, number: &NumberLiteral) -> Result<String> {
    if plain_number_round_trips(cx, number) {
        return Ok(number.canonical.clone());
    }
    encode_number_escape_lisp(cx, number)
}

fn encode_number_escape_lisp(cx: &mut WriteCx<'_>, number: &NumberLiteral) -> Result<String> {
    encode_escape_named(
        cx,
        "expr:number",
        &[
            Expr::Symbol(number.domain.clone()),
            Expr::String(number.canonical.clone()),
        ],
    )
}

fn plain_number_round_trips(cx: &mut WriteCx<'_>, number: &NumberLiteral) -> bool {
    let mut read_cx = ReadCx {
        cx: cx.cx,
        codec: cx.codec,
        read_policy: Default::default(),
        limits: DecodeLimits::default(),
    };
    LispProcMacroDecoder
        .decode(&mut read_cx, Input::Text(number.canonical.clone()))
        .is_ok_and(|expr| expr == Expr::Number(number.clone()))
}

fn encode_symbol_lisp(cx: &mut WriteCx<'_>, symbol: &Symbol) -> Result<String> {
    let text = symbol.to_string();
    if plain_symbol_round_trips(cx, symbol, &text)? {
        return Ok(text);
    }
    encode_symbol_escape_lisp(cx, symbol)
}

fn encode_symbol_escape_lisp(cx: &mut WriteCx<'_>, symbol: &Symbol) -> Result<String> {
    let namespace = match &symbol.namespace {
        Some(namespace) => Expr::String(namespace.to_string()),
        None => Expr::Nil,
    };
    encode_escape_named(
        cx,
        "expr:symbol",
        &[namespace, Expr::String(symbol.name.to_string())],
    )
}

fn encode_local_lisp(cx: &mut WriteCx<'_>, symbol: &Symbol) -> Result<String> {
    let namespace = match &symbol.namespace {
        Some(namespace) => Expr::String(namespace.to_string()),
        None => Expr::Nil,
    };
    encode_escape_named(
        cx,
        "expr:local",
        &[namespace, Expr::String(symbol.name.to_string())],
    )
}

fn plain_symbol_round_trips(cx: &mut WriteCx<'_>, symbol: &Symbol, text: &str) -> Result<bool> {
    if matches!(text, "nil" | "true" | "false") || parse_logic_var(text).is_some() {
        return Ok(false);
    }
    if !text.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '/' | ':' | '?' | '!' | '-' | '+' | '.')
    }) {
        return Ok(false);
    }
    if has_punctuation_digit_boundary(text) {
        return Ok(false);
    }
    if may_be_number_literal(text) && cx.cx.parse_number_literal(text)?.is_some() {
        return Ok(false);
    }
    Ok(parse_symbol(text) == *symbol)
}

fn starts_with_joining_punctuation(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|ch| matches!(ch, '/' | ':' | '?' | '!' | '-' | '+' | '.'))
}

fn ends_with_joining_punctuation(text: &str) -> bool {
    text.chars()
        .last()
        .is_some_and(|ch| matches!(ch, '/' | ':' | '?' | '!' | '-' | '+' | '.'))
}

fn has_punctuation_digit_boundary(text: &str) -> bool {
    let mut previous = None;
    for ch in text.chars() {
        if ch.is_ascii_digit()
            && previous.is_some_and(|prev| matches!(prev, '/' | ':' | '?' | '!' | '-' | '+' | '.'))
        {
            return true;
        }
        previous = Some(ch);
    }
    false
}

fn encode_lisp_tree(cx: &mut WriteCx<'_>, tree: &LocatedExprTree) -> Result<String> {
    let prefix = encode_trivia(&tree.origin);
    let body = match &tree.expr {
        Expr::List(_) => encode_tree_seq(cx, "(", ")", &tree.children)?,
        Expr::Vector(_) => encode_tree_seq(cx, "[", "]", &tree.children)?,
        Expr::Block(_) => encode_tree_seq(cx, "{", "}", &tree.children)?,
        Expr::Quote { mode, .. } if tree.children.len() == 1 => {
            let name = match mode {
                QuoteMode::Quote => "quote",
                QuoteMode::QuasiQuote => "quasiquote",
                QuoteMode::Unquote => "unquote",
                QuoteMode::Splice => "splice",
                QuoteMode::Syntax => "syntax",
            };
            format!(
                "({name} {})",
                encode_quote_tree_operand_lisp(cx, &tree.children[0])?
            )
        }
        _ => encode_lisp_expr(cx, &tree.expr)?,
    };
    Ok(format!("{prefix}{body}"))
}

fn encode_tree_seq(
    cx: &mut WriteCx<'_>,
    start: &str,
    end: &str,
    items: &[LocatedExprTree],
) -> Result<String> {
    let inner = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let previous = index.checked_sub(1).and_then(|prev| items.get(prev));
            let next = items.get(index + 1);
            encode_tree_seq_item(cx, item, previous, next)
        })
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    Ok(format!("{start}{inner}{end}"))
}

fn encode_seq(cx: &mut WriteCx<'_>, start: &str, end: &str, items: &[Expr]) -> Result<String> {
    let inner = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let previous = index.checked_sub(1).and_then(|prev| items.get(prev));
            let next = items.get(index + 1);
            encode_seq_item(cx, item, previous, next)
        })
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    Ok(format!("{start}{inner}{end}"))
}

fn encode_tree_seq_item(
    cx: &mut WriteCx<'_>,
    item: &LocatedExprTree,
    previous: Option<&LocatedExprTree>,
    next: Option<&LocatedExprTree>,
) -> Result<String> {
    if let Expr::Symbol(symbol) = &item.expr
        && symbol_needs_sequence_escape(
            symbol,
            previous.map(|tree| &tree.expr),
            next.map(|tree| &tree.expr),
        )
    {
        return Ok(format!(
            "{}{}",
            encode_trivia(&item.origin),
            encode_symbol_escape_lisp(cx, symbol)?
        ));
    }
    encode_lisp_tree(cx, item)
}

fn encode_seq_item(
    cx: &mut WriteCx<'_>,
    item: &Expr,
    previous: Option<&Expr>,
    next: Option<&Expr>,
) -> Result<String> {
    if let Expr::Symbol(symbol) = item
        && symbol_needs_sequence_escape(symbol, previous, next)
    {
        return encode_symbol_escape_lisp(cx, symbol);
    }
    encode_lisp_expr(cx, item)
}

fn symbol_needs_sequence_escape(
    symbol: &Symbol,
    previous: Option<&Expr>,
    next: Option<&Expr>,
) -> bool {
    let text = symbol.to_string();
    has_punctuation_digit_boundary(&text)
        || (starts_with_joining_punctuation(&text) && matches!(previous, Some(Expr::Symbol(_))))
        || (ends_with_joining_punctuation(&text) && matches!(next, Some(Expr::Symbol(_))))
}

fn encode_map(cx: &mut WriteCx<'_>, entries: &[(Expr, Expr)]) -> Result<String> {
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|(key, value)| (key.canonical_key(), value.canonical_key()));
    let items = sorted
        .iter()
        .map(|(key, value)| {
            Ok(format!(
                "[{} {}]",
                encode_map_key_expr(cx, key)?,
                encode_map_value_expr(cx, value)?
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    Ok(format!("(expr:map {items})"))
}

fn encode_map_key_expr(cx: &mut WriteCx<'_>, expr: &Expr) -> Result<String> {
    if let Expr::Symbol(symbol) = expr {
        let text = symbol.to_string();
        if ends_with_joining_punctuation(&text) || has_punctuation_digit_boundary(&text) {
            return encode_symbol_escape_lisp(cx, symbol);
        }
    }
    encode_lisp_expr(cx, expr)
}

fn encode_map_value_expr(cx: &mut WriteCx<'_>, expr: &Expr) -> Result<String> {
    if let Expr::Symbol(symbol) = expr {
        let text = symbol.to_string();
        if starts_with_joining_punctuation(&text) || has_punctuation_digit_boundary(&text) {
            return encode_symbol_escape_lisp(cx, symbol);
        }
    }
    encode_lisp_expr(cx, expr)
}

fn encode_escape_named(cx: &mut WriteCx<'_>, name: &str, args: &[Expr]) -> Result<String> {
    let mut items = Vec::with_capacity(args.len() + 1);
    items.push(Expr::Symbol(parse_symbol(name)));
    items.extend(args.iter().cloned());
    encode_seq(cx, "(", ")", &items)
}

fn encode_quote_lisp(cx: &mut WriteCx<'_>, mode: QuoteMode, expr: &Expr) -> Result<String> {
    let name = match mode {
        QuoteMode::Quote => "quote",
        QuoteMode::QuasiQuote => "quasiquote",
        QuoteMode::Unquote => "unquote",
        QuoteMode::Splice => "splice",
        QuoteMode::Syntax => "syntax",
    };

    let mut nested = cx.with_position(EncodePosition::Quote);
    let inner = encode_quote_operand_lisp(&mut nested, expr)?;
    Ok(format!("({name} {inner})"))
}

fn encode_quote_operand_lisp(cx: &mut WriteCx<'_>, expr: &Expr) -> Result<String> {
    if let Expr::Symbol(symbol) = expr {
        let text = symbol.to_string();
        if starts_with_joining_punctuation(&text) || has_punctuation_digit_boundary(&text) {
            return encode_symbol_escape_lisp(cx, symbol);
        }
    }
    encode_lisp_expr(cx, expr)
}

fn encode_quote_tree_operand_lisp(cx: &mut WriteCx<'_>, tree: &LocatedExprTree) -> Result<String> {
    if let Expr::Symbol(symbol) = &tree.expr {
        let text = symbol.to_string();
        if starts_with_joining_punctuation(&text) || has_punctuation_digit_boundary(&text) {
            return Ok(format!(
                "{}{}",
                encode_trivia(&tree.origin),
                encode_symbol_escape_lisp(cx, symbol)?
            ));
        }
    }
    encode_lisp_tree(cx, tree)
}

fn encode_byte_string_literal(bytes: &[u8]) -> String {
    let mut out = String::from("b\"");
    for byte in bytes {
        match byte {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            0x20..=0x7e => out.push(*byte as char),
            other => out.push_str(&format!("\\x{other:02x}")),
        }
    }
    out.push('"');
    out
}

fn encode_trivia(origin: &Option<Origin>) -> String {
    origin
        .as_ref()
        .map(|origin| {
            origin
                .trivia
                .iter()
                .map(|item| match item {
                    Trivia::Whitespace(text)
                    | Trivia::LineComment(text)
                    | Trivia::BlockComment(text) => text.clone(),
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Encodes a runtime [`Value`] to Lisp text via its object encoder, rendering
/// constructor, tagged-data, and opaque object encodings as s-expression forms.
pub fn encode_object_lisp(cx: &mut WriteCx<'_>, value: Value) -> Result<String> {
    let encoder = value
        .object()
        .as_object_encoder()
        .ok_or_else(|| sim_kernel::Error::Eval("value has no object encoder".to_owned()))?;
    match encoder.object_encoding(cx.cx)? {
        ObjectEncoding::Constructor { class, args } => encode_constructor_lisp(cx, class, args),
        ObjectEncoding::TaggedData { tag, fields } => {
            let mut items = vec![
                Expr::Symbol(Symbol::qualified("object", "tagged")),
                Expr::Symbol(tag),
            ];
            items.extend(
                fields
                    .into_iter()
                    .map(|(name, value)| Expr::List(vec![Expr::Symbol(name), value])),
            );
            encode_seq(cx, "(", ")", &items)
        }
        ObjectEncoding::Opaque { class, stable_id } => encode_seq(
            cx,
            "(",
            ")",
            &[
                Expr::Symbol(Symbol::qualified("object", "opaque")),
                Expr::Symbol(class),
                Expr::String(stable_id),
            ],
        ),
    }
}

fn encode_constructor_lisp(cx: &mut WriteCx<'_>, class: Symbol, args: Vec<Expr>) -> Result<String> {
    let mut items = Vec::with_capacity(args.len() + 1);
    items.push(Expr::Symbol(class.clone()));
    items.extend(args);
    let inner = items
        .iter()
        .map(|item| encode_lisp_expr(cx, item))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    match cx.options.position {
        EncodePosition::Eval => Ok(format!("({inner})")),
        EncodePosition::Quote | EncodePosition::Data
            if matches!(cx.options.read_construct, ReadConstructEncodePolicy::Allow) =>
        {
            Ok(format!("#({inner})"))
        }
        _ => Ok(format!("(object {inner})")),
    }
}
