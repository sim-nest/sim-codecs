//! Encoding for the Algol codec: renders any `Expr` (or located expression
//! tree) back to infix text, inserting parentheses according to operator
//! binding power drawn from the Pratt table.

use sim_codec::{Decoder, Encoder, Input, ReadCx, encode_string_literal};
use sim_codec_lisp::{LispProcMacroDecoder, LispProcMacroEncoder};
use sim_kernel::{
    EncodeOptions, Expr, LocatedExprTree, NumberLiteral, Origin, PrattTable, Result, Symbol,
    Trivia, WriteCx,
};

/// Renders an [`Expr`] back to Algol infix text using the operator `table`.
///
/// `parent_bp` is the binding power of the enclosing context; when an infix
/// operator binds more loosely than its parent the rendered subexpression is
/// wrapped in parentheses so the text reparses to the same tree. Forms outside
/// the infix grammar (collections, quotes, and similar) are emitted through a
/// `expr.lisp(...)` escape so the general-purpose codec still round-trips every
/// expression the kernel can hold.
///
/// # Examples
///
/// ```
/// use sim_codec_algol::{default_pratt_table, encode_algol};
/// use sim_kernel::{EncodeOptions, Expr, Symbol, WriteCx};
/// use sim_test_support::core_cx;
///
/// let mut cx = core_cx();
/// let table = default_pratt_table();
/// let expr = Expr::Infix {
///     operator: Symbol::new("+"),
///     left: Box::new(Expr::Symbol(Symbol::new("a"))),
///     right: Box::new(Expr::Symbol(Symbol::new("b"))),
/// };
/// let mut write = WriteCx {
///     cx: &mut cx,
///     codec: sim_kernel::CodecId(0),
///     options: EncodeOptions::default(),
/// };
/// assert_eq!(encode_algol(&expr, &table, 0, &mut write).unwrap(), "a + b");
/// ```
pub fn encode_algol(
    expr: &Expr,
    table: &PrattTable,
    parent_bp: u16,
    cx: &mut WriteCx<'_>,
) -> Result<String> {
    match expr {
        Expr::Nil => Ok("nil".to_owned()),
        Expr::Bool(true) => Ok("true".to_owned()),
        Expr::Bool(false) => Ok("false".to_owned()),
        Expr::Number(number) => encode_number_algol(number, table, cx),
        Expr::Symbol(symbol) => encode_symbol_algol(symbol, table, cx),
        Expr::String(value) => Ok(encode_string_literal(value)),
        Expr::Infix {
            operator,
            left,
            right,
        } => {
            let op = table.require_infix(operator)?;
            let lhs = encode_algol(left, table, op.left_bp, cx)?;
            let rhs = encode_algol(right, table, op.right_bp, cx)?;
            let text = format!("{} {} {}", lhs, operator, rhs);
            if op.left_bp < parent_bp {
                Ok(format!("({})", text))
            } else {
                Ok(text)
            }
        }
        Expr::Prefix { operator, arg } => {
            let op = table.require_prefix(operator)?;
            let inner = encode_algol(arg, table, op.right_bp, cx)?;
            Ok(format!("{}{}", operator, inner))
        }
        Expr::Postfix { operator, arg } => {
            let op = table.require_postfix(operator)?;
            let inner = encode_algol(arg, table, op.left_bp, cx)?;
            Ok(format!("{}{}", inner, operator))
        }
        Expr::Call { operator, args } => {
            let op = encode_algol(operator, table, 110, cx)?;
            let args = args
                .iter()
                .map(|arg| encode_algol(arg, table, 0, cx))
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            Ok(format!("{}({})", op, args))
        }
        other => encode_escape_algol(other, cx),
    }
}

fn encode_number_algol(
    number: &NumberLiteral,
    table: &PrattTable,
    cx: &mut WriteCx<'_>,
) -> Result<String> {
    let expr = Expr::Number(number.clone());
    if algol_text_round_trips(&expr, &number.canonical, table, cx) {
        return Ok(number.canonical.clone());
    }
    encode_escape_algol(&expr, cx)
}

fn encode_symbol_algol(
    symbol: &Symbol,
    table: &PrattTable,
    cx: &mut WriteCx<'_>,
) -> Result<String> {
    let text = symbol.to_string();
    let expr = Expr::Symbol(symbol.clone());
    if algol_text_round_trips(&expr, &text, table, cx) {
        return Ok(text);
    }
    encode_escape_algol(&expr, cx)
}

fn algol_text_round_trips(
    expected: &Expr,
    text: &str,
    table: &PrattTable,
    cx: &mut WriteCx<'_>,
) -> bool {
    crate::parse::parse_algol_expr_with_table(cx.cx, table.clone(), text)
        .is_ok_and(|parsed| parsed.canonical_eq(expected))
}

pub(crate) fn encode_algol_tree(
    tree: &LocatedExprTree,
    table: &PrattTable,
    parent_bp: u16,
    cx: &mut WriteCx<'_>,
) -> Result<String> {
    let prefix = encode_trivia(&tree.origin);
    let body = match &tree.expr {
        Expr::Infix { operator, .. } if tree.children.len() == 2 => {
            let op = table.require_infix(operator)?;
            let lhs = encode_algol_tree(&tree.children[0], table, op.left_bp, cx)?;
            let rhs = encode_algol_tree(&tree.children[1], table, op.right_bp, cx)?;
            let text = format!("{} {} {}", lhs, operator, rhs);
            if op.left_bp < parent_bp {
                format!("({})", text)
            } else {
                text
            }
        }
        Expr::Prefix { operator, .. } if tree.children.len() == 1 => {
            let op = table.require_prefix(operator)?;
            let inner = encode_algol_tree(&tree.children[0], table, op.right_bp, cx)?;
            format!("{}{}", operator, inner)
        }
        Expr::Postfix { operator, .. } if tree.children.len() == 1 => {
            let op = table.require_postfix(operator)?;
            let inner = encode_algol_tree(&tree.children[0], table, op.left_bp, cx)?;
            format!("{}{}", inner, operator)
        }
        Expr::Call { .. } if !tree.children.is_empty() => {
            let operator = encode_algol_tree(&tree.children[0], table, 110, cx)?;
            let args = tree.children[1..]
                .iter()
                .map(|arg| encode_algol_tree(arg, table, 0, cx))
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            format!("{}({})", operator, args)
        }
        _ => encode_algol(&tree.expr, table, parent_bp, cx)?,
    };
    Ok(format!("{prefix}{body}"))
}

fn encode_escape_algol(expr: &Expr, cx: &mut WriteCx<'_>) -> Result<String> {
    let mut nested = WriteCx {
        cx: &mut *cx.cx,
        codec: cx.codec,
        options: EncodeOptions::default(),
    };
    let text = LispProcMacroEncoder
        .encode(&mut nested, expr)?
        .into_text()?;
    Ok(format!("expr.lisp({})", encode_string_literal(&text)))
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

pub(crate) fn decode_escape(cx: &mut ReadCx<'_>, expr: Expr) -> Result<Expr> {
    match expr {
        Expr::Call { operator, args } => {
            if matches!(operator.as_ref(), Expr::Symbol(symbol) if symbol == &Symbol::qualified("expr", "lisp"))
            {
                let [Expr::String(text)] = args.as_slice() else {
                    return Err(sim_kernel::Error::Eval(
                        "expr.lisp expects one string arg".to_owned(),
                    ));
                };
                return LispProcMacroDecoder.decode(cx, Input::Text(text.clone()));
            }

            Ok(Expr::Call {
                operator,
                args: args
                    .into_iter()
                    .map(|arg| decode_escape(cx, arg))
                    .collect::<Result<Vec<_>>>()?,
            })
        }
        Expr::Infix {
            operator,
            left,
            right,
        } => Ok(Expr::Infix {
            operator,
            left: Box::new(decode_escape(cx, *left)?),
            right: Box::new(decode_escape(cx, *right)?),
        }),
        Expr::Prefix { operator, arg } => Ok(Expr::Prefix {
            operator,
            arg: Box::new(decode_escape(cx, *arg)?),
        }),
        Expr::Postfix { operator, arg } => Ok(Expr::Postfix {
            operator,
            arg: Box::new(decode_escape(cx, *arg)?),
        }),
        Expr::List(items) => Ok(Expr::List(
            items
                .into_iter()
                .map(|item| decode_escape(cx, item))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Vector(items) => Ok(Expr::Vector(
            items
                .into_iter()
                .map(|item| decode_escape(cx, item))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Map(entries) => Ok(Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| Ok((decode_escape(cx, key)?, decode_escape(cx, value)?)))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Set(items) => Ok(Expr::Set(
            items
                .into_iter()
                .map(|item| decode_escape(cx, item))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Block(items) => Ok(Expr::Block(
            items
                .into_iter()
                .map(|item| decode_escape(cx, item))
                .collect::<Result<Vec<_>>>()?,
        )),
        Expr::Quote { mode, expr } => Ok(Expr::Quote { mode, expr }),
        Expr::Annotated { expr, annotations } => Ok(Expr::Annotated {
            expr: Box::new(decode_escape(cx, *expr)?),
            annotations: annotations
                .into_iter()
                .map(|(name, value)| Ok((name, decode_escape(cx, value)?)))
                .collect::<Result<Vec<_>>>()?,
        }),
        Expr::Extension { tag, payload } => Ok(Expr::Extension {
            tag,
            payload: Box::new(decode_escape(cx, *payload)?),
        }),
        other => Ok(other),
    }
}
