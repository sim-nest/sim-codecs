use serde_json::Value;
use sim_codec_json::{JsonProjectionMode, project_expr_to_json};
use sim_kernel::{CodecId, Error, Expr, Result, Symbol};
use sim_value::access::{
    entry_field_any, entry_required_list_any, entry_required_str_any, entry_required_sym_any,
};

pub(super) fn codec_eval_to_codec(codec: CodecId, err: Error) -> Error {
    match err {
        Error::Eval(message) => codec_error(codec, message),
        other => other,
    }
}

pub(super) fn codec_error(codec: CodecId, message: impl ToString) -> Error {
    Error::CodecError {
        codec,
        message: message.to_string(),
    }
}

pub(super) fn expr_entries<'a>(
    expr: &'a Expr,
    context: &'static str,
) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        _ => Err(Error::Eval(format!("{context} must be a map"))),
    }
}

pub(super) fn required_symbol<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Result<&'a Symbol> {
    entry_required_sym_any(entries, key, "anthropic codec symbol field")
}

pub(super) fn required_string<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Result<&'a str> {
    entry_required_str_any(entries, key, "anthropic codec string field")
}

pub(super) fn required_list<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Result<&'a [Expr]> {
    entry_required_list_any(entries, key, "anthropic codec list field")
}

pub(super) fn optional_expr<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Option<&'a Expr> {
    entry_field_any(entries, key)
}

pub(super) fn optional_string(entries: &[(Expr, Expr)], key: &str) -> Result<Option<String>> {
    optional_expr(entries, key)
        .map(|expr| match expr {
            Expr::String(text) => Ok(text.clone()),
            Expr::Symbol(symbol) => Ok(symbol.as_qualified_str()),
            _ => Err(Error::Eval(format!(
                "anthropic codec field {key} must be a string or symbol"
            ))),
        })
        .transpose()
}

pub(super) fn provider_symbol(value: &str) -> Symbol {
    Symbol::new(value.replace('_', "-"))
}

pub(super) fn sim_expr_to_json(expr: &Expr) -> Value {
    project_expr_to_json(expr, JsonProjectionMode::UntaggedInterop)
}

pub(super) fn flatten_expr(expr: &Expr) -> String {
    match expr {
        Expr::Nil => "nil".to_owned(),
        Expr::Bool(flag) => flag.to_string(),
        Expr::Number(number) => number.canonical.clone(),
        Expr::Symbol(symbol) | Expr::Local(symbol) => symbol.to_string(),
        Expr::String(text) => text.clone(),
        Expr::Bytes(bytes) => format!("{bytes:?}"),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.iter().map(flatten_expr).collect::<Vec<_>>().join(" ")
        }
        Expr::Map(entries) => entries
            .iter()
            .map(|(key, value)| format!("{} {}", flatten_expr(key), flatten_expr(value)))
            .collect::<Vec<_>>()
            .join(" "),
        Expr::Call { operator, args } => std::iter::once(flatten_expr(operator))
            .chain(args.iter().map(flatten_expr))
            .collect::<Vec<_>>()
            .join(" "),
        Expr::Infix {
            operator,
            left,
            right,
        } => format!(
            "{} {} {}",
            flatten_expr(left),
            operator,
            flatten_expr(right)
        ),
        Expr::Prefix { operator, arg } => format!("{operator} {}", flatten_expr(arg)),
        Expr::Postfix { operator, arg } => format!("{} {operator}", flatten_expr(arg)),
        Expr::Quote { expr, .. } | Expr::Annotated { expr, .. } => flatten_expr(expr),
        Expr::Extension { tag, payload } => format!("{tag} {}", flatten_expr(payload)),
    }
}
