//! Parsing helpers for individual Lisp forms: symbols, logic variables, string
//! and byte-string literals, number literals, and explicit quote/quasiquote
//! forms, turning lexed atoms into the matching `Expr` shapes.

use sim_codec::{ReadCx, decode_string_literal};
use sim_kernel::{Error, Expr, NumberLiteral, QuoteMode, Result, Symbol, Value};

pub(crate) fn parse_symbol(raw: &str) -> Symbol {
    match raw.rsplit_once('/') {
        Some((namespace, name)) => Symbol::qualified(namespace.to_owned(), name.to_owned()),
        None => Symbol::new(raw.to_owned()),
    }
}

pub(crate) fn parse_logic_var(raw: &str) -> Option<Expr> {
    if !raw.starts_with('?') {
        return None;
    }
    let name = raw.strip_prefix('?')?;
    if name.is_empty() {
        return None;
    }
    if name == "_" {
        return Some(Expr::Local(Symbol::new("_")));
    }
    Some(Expr::Local(Symbol::new(name.to_owned())))
}

pub(crate) fn read_explicit_quote(items: &[Expr]) -> Option<Expr> {
    let [Expr::Symbol(symbol), expr] = items else {
        return None;
    };
    let mode = match (symbol.namespace.as_deref(), symbol.name.as_ref()) {
        (None, "quote") => QuoteMode::Quote,
        (None, "quasiquote") => QuoteMode::QuasiQuote,
        (None, "unquote") => QuoteMode::Unquote,
        (None, "splice") => QuoteMode::Splice,
        (None, "syntax") => QuoteMode::Syntax,
        _ => return None,
    };
    Some(Expr::Quote {
        mode,
        expr: Box::new(expr.clone()),
    })
}

pub(crate) fn read_escape_form(items: &[Expr]) -> Result<Option<Expr>> {
    let Some(Expr::Symbol(head)) = items.first() else {
        return Ok(None);
    };
    let Some(head_name) = escape_head_name(head) else {
        return Ok(None);
    };

    match head_name {
        "expr:map" => Ok(Some(Expr::Map(
            items[1..]
                .iter()
                .map(|entry| match entry {
                    Expr::Vector(parts) | Expr::List(parts) if parts.len() == 2 => {
                        Ok((parts[0].clone(), parts[1].clone()))
                    }
                    _ => Err(Error::Eval(format!(
                        "expr:map entries must be [key value] vectors, found {entry:?}"
                    ))),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        "expr:set" => Ok(Some(Expr::Set(items[1..].to_vec()))),
        "expr:call" => match items {
            [_, operator, args @ ..] => Ok(Some(Expr::Call {
                operator: Box::new(operator.clone()),
                args: args.to_vec(),
            })),
            _ => Err(Error::Eval(
                "expr:call expects (expr:call operator arg ...)".to_owned(),
            )),
        },
        "expr:infix" => match items {
            [_, operator, left, right] => Ok(Some(Expr::Infix {
                operator: escape_operator_symbol(operator)?,
                left: Box::new(left.clone()),
                right: Box::new(right.clone()),
            })),
            _ => Err(Error::Eval(
                "expr:infix expects (expr:infix operator left right)".to_owned(),
            )),
        },
        "expr:prefix" => match items {
            [_, operator, arg] => Ok(Some(Expr::Prefix {
                operator: escape_operator_symbol(operator)?,
                arg: Box::new(arg.clone()),
            })),
            _ => Err(Error::Eval(
                "expr:prefix expects (expr:prefix operator arg)".to_owned(),
            )),
        },
        "expr:postfix" => match items {
            [_, operator, arg] => Ok(Some(Expr::Postfix {
                operator: escape_operator_symbol(operator)?,
                arg: Box::new(arg.clone()),
            })),
            _ => Err(Error::Eval(
                "expr:postfix expects (expr:postfix operator arg)".to_owned(),
            )),
        },
        "expr:annotated" => match items {
            [_, expr, annotations @ ..] => Ok(Some(Expr::Annotated {
                expr: Box::new(expr.clone()),
                annotations: annotations
                    .iter()
                    .map(|entry| match entry {
                        Expr::List(items) if items.len() == 2 => match &items[0] {
                            Expr::Symbol(symbol) => Ok((symbol.clone(), items[1].clone())),
                            _ => Err(Error::Eval(
                                "expr:annotated entries must start with a symbol".to_owned(),
                            )),
                        },
                        _ => Err(Error::Eval(
                            "expr:annotated entries must be (name value) lists".to_owned(),
                        )),
                    })
                    .collect::<Result<Vec<_>>>()?,
            })),
            _ => Err(Error::Eval(
                "expr:annotated expects (expr:annotated expr (name value) ...)".to_owned(),
            )),
        },
        "expr:extension" => match items {
            [_, Expr::Symbol(tag), payload] => Ok(Some(Expr::Extension {
                tag: tag.clone(),
                payload: Box::new(payload.clone()),
            })),
            _ => Err(Error::Eval(
                "expr:extension expects (expr:extension tag payload)".to_owned(),
            )),
        },
        "expr:number" => match items {
            [_, Expr::Symbol(domain), Expr::String(canonical)] => {
                Ok(Some(Expr::Number(NumberLiteral {
                    domain: domain.clone(),
                    canonical: canonical.clone(),
                })))
            }
            _ => Err(Error::Eval(
                "expr:number expects (expr:number domain canonical-string)".to_owned(),
            )),
        },
        "expr:local" => match items {
            [_, Expr::Nil, Expr::String(name)] => Ok(Some(Expr::Local(Symbol::new(name.clone())))),
            [_, Expr::String(namespace), Expr::String(name)] => Ok(Some(Expr::Local(
                Symbol::qualified(namespace.clone(), name.clone()),
            ))),
            _ => Err(Error::Eval(
                "expr:local expects (expr:local namespace-or-nil name)".to_owned(),
            )),
        },
        "expr:symbol" => match items {
            [_, Expr::Nil, Expr::String(name)] => Ok(Some(Expr::Symbol(Symbol::new(name.clone())))),
            [_, Expr::String(namespace), Expr::String(name)] => Ok(Some(Expr::Symbol(
                Symbol::qualified(namespace.clone(), name.clone()),
            ))),
            _ => Err(Error::Eval(
                "expr:symbol expects (expr:symbol namespace-or-nil name)".to_owned(),
            )),
        },
        _ => Ok(None),
    }
}

fn escape_head_name(symbol: &Symbol) -> Option<&str> {
    match (symbol.namespace.as_deref(), symbol.name.as_ref()) {
        (None, name) if name.starts_with("expr:") => Some(name),
        (Some("expr"), "annotated") => Some("expr:annotated"),
        _ => None,
    }
}

pub(crate) fn may_be_number_literal(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let starts_like_number = match first {
        ch if ch.is_ascii_digit() => true,
        '+' | '-' | '.' => chars.next().is_some_and(|ch| ch.is_ascii_digit()),
        _ => false,
    };
    starts_like_number
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+' | '/' | '_'))
        && raw.chars().any(|ch| ch.is_ascii_digit())
}

pub(crate) fn parse_string_literal(codec: sim_kernel::CodecId, raw: &str) -> Result<String> {
    decode_string_literal(codec, raw)
}

fn escape_operator_symbol(expr: &Expr) -> Result<Symbol> {
    match expr {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        Expr::String(text) => Ok(Symbol::new(text.clone())),
        _ => Err(Error::Eval(
            "escape operator must be a symbol or string".to_owned(),
        )),
    }
}

pub(crate) fn parse_byte_string_literal(raw: &str) -> Result<Vec<u8>> {
    let inner = raw
        .strip_prefix("b\"")
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or_else(|| Error::Eval(format!("invalid byte string literal {raw}")))?;
    let chars = inner.chars().collect::<Vec<_>>();
    let mut index = 0;
    let mut out = Vec::new();

    while index < chars.len() {
        let ch = chars[index];
        if ch != '\\' {
            if ch.is_ascii() {
                out.push(ch as u8);
                index += 1;
                continue;
            }
            return Err(Error::Eval(format!(
                "non-ascii byte literal content must be escaped in {raw}"
            )));
        }

        index += 1;
        let escaped = chars
            .get(index)
            .copied()
            .ok_or_else(|| Error::Eval(format!("unterminated byte string escape {raw}")))?;
        match escaped {
            'n' => out.push(b'\n'),
            'r' => out.push(b'\r'),
            't' => out.push(b'\t'),
            '"' => out.push(b'"'),
            '\\' => out.push(b'\\'),
            'x' => {
                let hi = chars
                    .get(index + 1)
                    .copied()
                    .ok_or_else(|| Error::Eval(format!("invalid hex escape in {raw}")))?;
                let lo = chars
                    .get(index + 2)
                    .copied()
                    .ok_or_else(|| Error::Eval(format!("invalid hex escape in {raw}")))?;
                let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16)
                    .map_err(|_| Error::Eval(format!("invalid hex escape in {raw}")))?;
                out.push(byte);
                index += 2;
            }
            other => {
                return Err(Error::Eval(format!(
                    "unsupported byte string escape \\{other} in {raw}"
                )));
            }
        }
        index += 1;
    }

    Ok(out)
}

pub(crate) fn lower_eval_surface(expr: Expr) -> Expr {
    match expr {
        Expr::List(items) if items.len() > 1 => {
            let mut items = items
                .into_iter()
                .map(lower_eval_surface)
                .collect::<Vec<_>>();
            let operator = Box::new(items.remove(0));
            Expr::Call {
                operator,
                args: items,
            }
        }
        Expr::List(items) => Expr::List(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Vector(items) => Expr::Vector(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Map(entries) => Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| (lower_eval_surface(key), lower_eval_surface(value)))
                .collect(),
        ),
        Expr::Set(items) => Expr::Set(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Block(items) => Expr::Block(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Quote { mode, expr } => Expr::Quote { mode, expr },
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(lower_eval_surface(*expr)),
            annotations: annotations
                .into_iter()
                .map(|(name, value)| (name, lower_eval_surface(value)))
                .collect(),
        },
        Expr::Extension { tag, payload } => Expr::Extension {
            tag,
            payload: Box::new(lower_eval_surface(*payload)),
        },
        other => other,
    }
}

pub(crate) fn decode_data_expr(cx: &mut ReadCx<'_>, expr: Expr) -> Result<Value> {
    match expr {
        Expr::Nil => cx.cx.factory().nil(),
        Expr::Bool(value) => cx.cx.factory().bool(value),
        Expr::Number(number) => cx
            .cx
            .factory()
            .number_literal(number.domain, number.canonical),
        Expr::Symbol(symbol) => cx.cx.factory().symbol(symbol),
        Expr::Local(_) => cx.cx.factory().expr(expr),
        Expr::String(value) => cx.cx.factory().string(value),
        Expr::Bytes(value) => cx.cx.factory().bytes(value),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) => {
            let values = items
                .into_iter()
                .map(|item| decode_data_expr(cx, item))
                .collect::<Result<Vec<_>>>()?;
            cx.cx.factory().list(values)
        }
        Expr::Map(entries) => {
            let entries = entries
                .into_iter()
                .map(|(key, value)| {
                    let Expr::Symbol(key) = key else {
                        return Err(Error::TypeMismatch {
                            expected: "symbol key",
                            found: "non-symbol key",
                        });
                    };
                    Ok((key, decode_data_expr(cx, value)?))
                })
                .collect::<Result<Vec<_>>>()?;
            cx.cx.factory().table(entries)
        }
        Expr::Quote { .. }
        | Expr::Call { .. }
        | Expr::Infix { .. }
        | Expr::Prefix { .. }
        | Expr::Postfix { .. }
        | Expr::Block(_)
        | Expr::Annotated { .. }
        | Expr::Extension { .. } => cx.cx.factory().expr(expr),
    }
}
