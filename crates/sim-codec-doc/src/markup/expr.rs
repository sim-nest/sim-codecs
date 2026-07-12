use std::collections::BTreeMap;

use sim_kernel::{Error, Expr, NumberLiteral, Result};
use sim_value::build::{entry, list, sym, text};
use sim_value::kind::expr_kind;

use crate::document::DocFormat;

use super::{Inline, MarkupBlock, Span};

pub(super) fn format_name(format: DocFormat) -> &'static str {
    match format {
        DocFormat::Text => "text",
        DocFormat::Markdown => "markdown",
    }
}

pub(super) fn attrs_expr(attrs: &BTreeMap<String, Expr>) -> Expr {
    Expr::Map(
        attrs
            .iter()
            .map(|(name, value)| (sym(name), value.clone()))
            .collect(),
    )
}

pub(super) fn attrs_from_entries(entries: &[(Expr, Expr)]) -> Result<BTreeMap<String, Expr>> {
    let mut attrs = BTreeMap::new();
    for (key, value) in entries {
        let name = key_name(key)
            .ok_or_else(|| Error::Eval("markup attr keys must be strings or symbols".to_owned()))?;
        attrs.insert(name, value.clone());
    }
    Ok(attrs)
}

pub(super) fn block_list(blocks: &[MarkupBlock]) -> Expr {
    list(blocks.iter().map(MarkupBlock::as_expr).collect())
}

pub(super) fn block_vec(items: &[Expr]) -> Result<Vec<MarkupBlock>> {
    items.iter().map(MarkupBlock::from_expr).collect()
}

pub(super) fn inline_list(items: &[Inline]) -> Expr {
    list(items.iter().map(Inline::as_expr).collect())
}

pub(super) fn inline_vec(items: &[Expr]) -> Result<Vec<Inline>> {
    items.iter().map(Inline::from_expr).collect()
}

pub(super) fn push_optional_string(
    entries: &mut Vec<(Expr, Expr)>,
    name: &str,
    value: &Option<String>,
) {
    if let Some(value) = value {
        entries.push(entry(name, text(value)));
    }
}

pub(super) fn push_optional_span(entries: &mut Vec<(Expr, Expr)>, span: &Option<Span>) {
    if let Some(span) = span {
        entries.push(entry("span", span.as_expr()));
    }
}

pub(super) fn optional_span(entries: &[(Expr, Expr)]) -> Result<Option<Span>> {
    field(entries, "span").map(Span::from_expr).transpose()
}

pub(super) fn blocks_to_source(blocks: &[MarkupBlock]) -> String {
    let mut out = String::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.push_str("\n\n");
        }
        block.write_source(&mut out);
    }
    out
}

pub(super) fn write_table_row(out: &mut String, row: &[Vec<Inline>]) {
    out.push('|');
    for cell in row {
        out.push(' ');
        write_inlines(out, cell);
        out.push_str(" |");
    }
}

pub(super) fn write_table_separator(out: &mut String, width: usize) {
    out.push('|');
    for _ in 0..width {
        out.push_str(" --- |");
    }
}

pub(super) fn write_inlines(out: &mut String, items: &[Inline]) {
    for item in items {
        match item {
            Inline::Text(value) => out.push_str(value),
            Inline::Emph(children) => {
                out.push('*');
                write_inlines(out, children);
                out.push('*');
            }
            Inline::Strong(children) => {
                out.push_str("**");
                write_inlines(out, children);
                out.push_str("**");
            }
            Inline::Code(value) => {
                out.push('`');
                out.push_str(value);
                out.push('`');
            }
            Inline::Link { label, target } => {
                out.push('[');
                write_inlines(out, label);
                out.push_str("](");
                out.push_str(target);
                out.push(')');
            }
            Inline::Math(source) => {
                out.push('$');
                out.push_str(&source.text);
                out.push('$');
            }
            Inline::Raw { text, .. } => out.push_str(text),
        }
    }
}

pub(super) fn map_entries<'a>(
    expr: &'a Expr,
    expected: &'static str,
) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        other => Err(Error::TypeMismatch {
            expected,
            found: expr_kind(other),
        }),
    }
}

pub(super) fn as_list<'a>(expr: &'a Expr, expected: &str) -> Result<&'a [Expr]> {
    match expr {
        Expr::List(items) => Ok(items),
        other => Err(Error::Eval(format!(
            "{expected} must be a list, found {}",
            expr_kind(other)
        ))),
    }
}

pub(super) fn required_field<'a>(
    entries: &'a [(Expr, Expr)],
    name: &str,
    context: &str,
) -> Result<&'a Expr> {
    field(entries, name).ok_or_else(|| Error::Eval(format!("{context} requires {name} field")))
}

pub(super) fn required_kind(entries: &[(Expr, Expr)], context: &str) -> Result<String> {
    required_symbol(entries, "kind", context)
}

pub(super) fn require_kind(entries: &[(Expr, Expr)], expected: &str, context: &str) -> Result<()> {
    let actual = required_kind(entries, context)?;
    if actual == expected {
        Ok(())
    } else {
        Err(Error::Eval(format!("{context} kind must be {expected}")))
    }
}

pub(super) fn required_symbol(
    entries: &[(Expr, Expr)],
    name: &str,
    context: &str,
) -> Result<String> {
    match required_field(entries, name, context)? {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Ok(symbol.name.to_string()),
        Expr::String(value) => Ok(value.clone()),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a symbol"
        ))),
    }
}

pub(super) fn required_string<'a>(
    entries: &'a [(Expr, Expr)],
    name: &str,
    context: &str,
) -> Result<&'a str> {
    match required_field(entries, name, context)? {
        Expr::String(value) => Ok(value),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a string"
        ))),
    }
}

pub(super) fn optional_string<'a>(
    entries: &'a [(Expr, Expr)],
    name: &str,
) -> Result<Option<&'a str>> {
    match field(entries, name) {
        Some(Expr::String(value)) => Ok(Some(value)),
        Some(_) => Err(Error::Eval(format!("{name} field must be a string"))),
        None => Ok(None),
    }
}

pub(super) fn required_list<'a>(
    entries: &'a [(Expr, Expr)],
    name: &str,
    context: &str,
) -> Result<&'a [Expr]> {
    as_list(required_field(entries, name, context)?, context)
}

pub(super) fn required_bool(entries: &[(Expr, Expr)], name: &str, context: &str) -> Result<bool> {
    match required_field(entries, name, context)? {
        Expr::Bool(value) => Ok(*value),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a bool"
        ))),
    }
}

pub(super) fn required_usize(entries: &[(Expr, Expr)], name: &str, context: &str) -> Result<usize> {
    match required_field(entries, name, context)? {
        Expr::Number(NumberLiteral { canonical, .. }) => canonical
            .parse()
            .map_err(|_| Error::Eval(format!("{context} field {name} must be an integer"))),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a number"
        ))),
    }
}

pub(super) fn required_u8(entries: &[(Expr, Expr)], name: &str, context: &str) -> Result<u8> {
    let value = required_usize(entries, name, context)?;
    u8::try_from(value).map_err(|_| Error::Eval(format!("{context} field {name} is too large")))
}

pub(super) fn field<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    entries
        .iter()
        .find_map(|(key, value)| (key_name(key).as_deref() == Some(name)).then_some(value))
}

fn key_name(key: &Expr) -> Option<String> {
    match key {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Some(symbol.name.to_string()),
        Expr::String(value) => Some(value.clone()),
        _ => None,
    }
}
