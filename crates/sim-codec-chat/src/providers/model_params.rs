use serde_json::{Map, Value};
use sim_kernel::{Error, Expr, Result};

pub(in crate::providers) fn attach_bridge_model_params(
    entries: &[(Expr, Expr)],
    payload: &mut Map<String, Value>,
    reserved_fields: &[&str],
    provider: &str,
) -> Result<()> {
    let Some(calls) = optional_field(entries, "bridge-calls") else {
        return Ok(());
    };
    for call in list_items(calls, "bridge-calls")? {
        let call_entries = map_entries(call, "bridge call")?;
        let Some(params) = optional_field(call_entries, "model-params") else {
            continue;
        };
        for (key, value) in map_entries(params, "bridge model-params")? {
            let key = model_param_key(key)?;
            if reserved_fields.contains(&key.as_str()) {
                return Err(Error::Eval(format!(
                    "{provider} model parameter {key} cannot override provider request field"
                )));
            }
            payload.insert(key, model_param_value(value));
        }
    }
    Ok(())
}

fn optional_field<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    entries.iter().find_map(|(key, value)| match key {
        Expr::Symbol(symbol) | Expr::Local(symbol) if symbol.name.as_ref() == name => Some(value),
        Expr::String(text) if text == name => Some(value),
        _ => None,
    })
}

fn list_items<'a>(expr: &'a Expr, context: &str) -> Result<&'a [Expr]> {
    match expr {
        Expr::List(items) | Expr::Vector(items) => Ok(items),
        _ => Err(Error::Eval(format!("{context} must be a list"))),
    }
}

fn map_entries<'a>(expr: &'a Expr, context: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        _ => Err(Error::Eval(format!("{context} must be a map"))),
    }
}

fn model_param_key(expr: &Expr) -> Result<String> {
    match expr {
        Expr::Symbol(symbol) | Expr::Local(symbol) => {
            Ok(symbol.as_qualified_str().replace('-', "_"))
        }
        Expr::String(text) => Ok(text.replace('-', "_")),
        other => Err(Error::Eval(format!(
            "bridge model parameter key must be a symbol or string, found {other:?}"
        ))),
    }
}

fn model_param_value(expr: &Expr) -> Value {
    match expr {
        Expr::Nil => Value::Null,
        Expr::Bool(flag) => Value::Bool(*flag),
        Expr::Number(number) => json_number(&number.canonical)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(number.canonical.clone())),
        Expr::String(text) => json_number(text)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(text.clone())),
        Expr::Symbol(symbol) | Expr::Local(symbol) => Value::String(symbol.as_qualified_str()),
        Expr::List(items) | Expr::Vector(items) => {
            Value::Array(items.iter().map(model_param_value).collect())
        }
        Expr::Map(entries) => {
            let mut object = Map::new();
            for (key, value) in entries {
                let key = match model_param_key(key) {
                    Ok(key) => key,
                    Err(_) => format!("{key:?}"),
                };
                object.insert(key, model_param_value(value));
            }
            Value::Object(object)
        }
        other => Value::String(format!("{other:?}")),
    }
}

fn json_number(text: &str) -> Option<serde_json::Number> {
    let Ok(Value::Number(number)) = serde_json::from_str::<Value>(text) else {
        return None;
    };
    Some(number)
}
