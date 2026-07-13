use serde_json::Value;
use sim_codec_json::json_number_to_u64;
use sim_kernel::{CodecId, Error, Expr, Result};
use sim_value::access::{entry_field, entry_required_str_any, entry_required_sym_any};

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

pub(super) fn marker_is_true(expr: &Expr, name: &str) -> bool {
    let Expr::Map(entries) = expr else {
        return false;
    };
    entries.iter().any(|(key, value)| {
        matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == name)
            && matches!(value, Expr::Bool(true))
    })
}

pub(super) fn symbol_field(entries: &[(Expr, Expr)], key: &str) -> Result<String> {
    entry_required_sym_any(entries, key, "openai codec symbol field")
        .map(|symbol| symbol.name.as_ref().to_owned())
}

pub(super) fn string_field(entries: &[(Expr, Expr)], key: &str) -> Result<String> {
    entry_required_str_any(entries, key, "openai codec string field").map(str::to_owned)
}

pub(super) fn list_field(expr: &Expr) -> Result<&[Expr]> {
    match expr {
        Expr::List(items) => Ok(items),
        _ => Err(Error::Eval("openai codec field must be a list".to_owned())),
    }
}

pub(super) fn map_field<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Result<&'a Expr> {
    entry_field(entries, key)
        .ok_or_else(|| Error::Eval(format!("openai codec missing {key} field")))
}

pub(super) fn optional_u64_field(entries: &[(Expr, Expr)], key: &str) -> Result<Option<u64>> {
    let Some(value) = entries.iter().find_map(|(field, value)| match field {
        Expr::Symbol(symbol) if symbol.name.as_ref() == key => Some(value),
        _ => None,
    }) else {
        return Ok(None);
    };
    match value {
        Expr::Number(number) => number
            .canonical
            .parse::<u64>()
            .map(Some)
            .map_err(|err| Error::Eval(format!("openai codec invalid {key}: {err}"))),
        other => {
            let json_number = match other {
                Expr::String(text) => serde_json::from_str::<Value>(text).ok(),
                _ => None,
            };
            json_number
                .as_ref()
                .and_then(json_number_to_u64)
                .ok_or_else(|| Error::Eval(format!("openai codec field {key} must be a number")))
                .map(Some)
        }
    }
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
