use std::{fs, path::PathBuf};

use sim_codec::{Input, decode_with_codec};
use sim_kernel::{
    Args, Callable, Cx, Error, Expr, Object, ObjectCompat, ReadPolicy, Result, Symbol, Table, Value,
};

use super::forms::lower_eval_surface;

const CLI_MAIN_NAME: &str = "main/codec-lisp";

pub(super) fn cli_main_symbol() -> Symbol {
    Symbol::qualified("cli", CLI_MAIN_NAME)
}

#[derive(Clone)]
pub(super) struct LispCliEntrypoint;

impl Object for LispCliEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(cli_main_symbol().to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LispCliEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LispCliEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let [envelope] = args.values() else {
            return Err(Error::Eval(
                "lisp cli entrypoint expects one envelope argument".to_owned(),
            ));
        };
        run_lisp_cli(cx, envelope)
    }
}

fn run_lisp_cli(cx: &mut Cx, envelope: &Value) -> Result<Value> {
    let envelope = envelope_table(envelope)?;
    let args = string_list_field(cx, envelope, "args")?;
    let eval = optional_string_field(cx, envelope, "eval")?;
    let script = optional_string_field(cx, envelope, "script")?;
    let stdin = optional_string_field(cx, envelope, "stdin")?;

    if !args.is_empty() {
        return Err(Error::Eval(format!(
            "lisp cli entrypoint does not support payload args: {}",
            args.join(" ")
        )));
    }

    let selected = selected_source(eval, script, stdin)?;
    match selected {
        Some(source) => eval_lisp_source(cx, source),
        None => cx.factory().symbol(Symbol::qualified("cli", "repl")),
    }
}

fn selected_source(
    eval: Option<String>,
    script: Option<String>,
    stdin: Option<String>,
) -> Result<Option<String>> {
    let selected = [eval.is_some(), script.is_some(), stdin.is_some()]
        .into_iter()
        .filter(|set| *set)
        .count();
    if selected > 1 {
        return Err(Error::Eval(
            "lisp cli entrypoint accepts only one of eval, script, or stdin".to_owned(),
        ));
    }
    if let Some(source) = eval {
        return Ok(Some(source));
    }
    if let Some(path) = script {
        return fs::read_to_string(PathBuf::from(path))
            .map(Some)
            .map_err(|err| Error::Eval(format!("read lisp script: {err}")));
    }
    Ok(stdin)
}

fn eval_lisp_source(cx: &mut Cx, source: String) -> Result<Value> {
    let expr = decode_with_codec(
        cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text(source),
        ReadPolicy::default(),
    )?;
    cx.eval_expr(lower_eval_surface(expr))
}

fn envelope_table(value: &Value) -> Result<&dyn Table> {
    value.object().as_table_impl().ok_or(Error::TypeMismatch {
        expected: "cli envelope table",
        found: "non-table",
    })
}

fn optional_string_field(cx: &mut Cx, table: &dyn Table, field: &str) -> Result<Option<String>> {
    match table.get(cx, Symbol::new(field))?.object().as_expr(cx)? {
        Expr::Nil => Ok(None),
        Expr::String(value) => Ok(Some(value)),
        _ => Err(Error::TypeMismatch {
            expected: "string or nil",
            found: "non-string",
        }),
    }
}

fn string_list_field(cx: &mut Cx, table: &dyn Table, field: &str) -> Result<Vec<String>> {
    let value = table.get(cx, Symbol::new(field))?;
    let Some(list) = value.object().as_list() else {
        return Err(Error::TypeMismatch {
            expected: "list",
            found: "non-list",
        });
    };
    list.to_vec(cx, Some(1024))?
        .into_iter()
        .map(|value| match value.object().as_expr(cx)? {
            Expr::String(value) => Ok(value),
            _ => Err(Error::TypeMismatch {
                expected: "string",
                found: "non-string",
            }),
        })
        .collect()
}
