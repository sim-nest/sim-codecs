use std::{fs, path::PathBuf};

use super::*;
use crate::implementation::cli::cli_main_symbol;
use sim_kernel::Lib;

#[derive(Clone)]
struct AddFunction;

impl Object for AddFunction {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("+".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for AddFunction {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for AddFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> sim_kernel::Result<Value> {
        let mut total = 0i64;
        for value in args.values() {
            let Expr::Number(number) = value.object().as_expr(cx)? else {
                return Err(sim_kernel::Error::TypeMismatch {
                    expected: "number",
                    found: "non-number",
                });
            };
            total += number
                .canonical
                .parse::<i64>()
                .map_err(|_| sim_kernel::Error::Eval("expected integer number".to_owned()))?;
        }
        cx.factory()
            .number_literal(Symbol::qualified("numbers", "f64"), total.to_string())
    }
}

#[test]
fn manifest_declares_lisp_cli_entrypoint() {
    let mut cx = cx();
    let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    let manifest = lib.manifest();

    assert!(
        manifest
            .exports
            .iter()
            .any(|export| export.symbol() == &cli_main_symbol())
    );

    cx.load_lib(&lib).unwrap();
    assert!(
        cx.registry()
            .function_by_symbol(&cli_main_symbol())
            .is_some()
    );
}

#[test]
fn eval_mode_returns_evaluated_value() {
    let mut cx = cx_with_lisp_cli();
    register_add(&mut cx);
    let envelope = envelope(&mut cx, &[], Some("(+ 1 2)"), None, None);

    let value = cx
        .call_function(&cli_main_symbol(), Args::new(vec![envelope]))
        .unwrap();

    assert_eq!(value.object().display(&mut cx).unwrap(), "3");
}

#[test]
fn script_mode_reads_and_evaluates_file() {
    let mut cx = cx_with_lisp_cli();
    register_add(&mut cx);
    let script = temp_script("(+ 4 5)");
    let envelope = envelope(
        &mut cx,
        &[],
        None,
        Some(script.to_str().expect("temp path is utf-8")),
        None,
    );

    let value = cx
        .call_function(&cli_main_symbol(), Args::new(vec![envelope]))
        .unwrap();

    fs::remove_file(script).unwrap();
    assert_eq!(value.object().display(&mut cx).unwrap(), "9");
}

#[test]
fn stdin_mode_evaluates_scripted_input() {
    let mut cx = cx_with_lisp_cli();
    register_add(&mut cx);
    let envelope = envelope(&mut cx, &[], None, None, Some("(+ 10 5)"));

    let value = cx
        .call_function(&cli_main_symbol(), Args::new(vec![envelope]))
        .unwrap();

    assert_eq!(value.object().display(&mut cx).unwrap(), "15");
}

#[test]
fn bare_handoff_enters_loaded_repl_behavior() {
    let mut cx = cx_with_lisp_cli();
    let envelope = envelope(&mut cx, &[], None, None, None);

    let value = cx
        .call_function(&cli_main_symbol(), Args::new(vec![envelope]))
        .unwrap();

    assert_eq!(value.object().display(&mut cx).unwrap(), "cli/repl");
}

#[test]
fn mixed_modes_fail_closed() {
    let mut cx = cx_with_lisp_cli();
    let envelope = envelope(&mut cx, &[], Some("1"), None, Some("2"));

    let err = cx
        .call_function(&cli_main_symbol(), Args::new(vec![envelope]))
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("only one of eval, script, or stdin")
    );
}

#[test]
fn unsupported_payload_args_fail_closed() {
    let mut cx = cx_with_lisp_cli();
    let envelope = envelope(&mut cx, &["server"], None, None, None);

    let err = cx
        .call_function(&cli_main_symbol(), Args::new(vec![envelope]))
        .unwrap_err();

    assert!(err.to_string().contains("does not support payload args"));
}

fn cx_with_lisp_cli() -> Cx {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    cx
}

fn register_add(cx: &mut Cx) {
    let value = cx.factory().opaque(Arc::new(AddFunction)).unwrap();
    cx.registry_mut()
        .register_function_value(Symbol::new("+"), value)
        .unwrap();
}

fn envelope(
    cx: &mut Cx,
    args: &[&str],
    eval: Option<&str>,
    script: Option<&str>,
    stdin: Option<&str>,
) -> Value {
    let verb = args
        .first()
        .map(|arg| cx.factory().string((*arg).to_owned()).unwrap())
        .unwrap_or_else(|| cx.factory().nil().unwrap());
    let args = cx
        .factory()
        .list(
            args.iter()
                .map(|arg| cx.factory().string((*arg).to_owned()).unwrap())
                .collect(),
        )
        .unwrap();
    let codec = cx
        .factory()
        .symbol(Symbol::qualified("codec", "lisp"))
        .unwrap();
    let eval = optional_string(cx, eval);
    let script = optional_string(cx, script);
    let stdin = optional_string(cx, stdin);
    cx.factory()
        .table(vec![
            (Symbol::new("codec"), codec),
            (Symbol::new("verb"), verb),
            (Symbol::new("args"), args),
            (Symbol::new("eval"), eval),
            (Symbol::new("script"), script),
            (Symbol::new("stdin"), stdin),
        ])
        .unwrap()
}

fn optional_string(cx: &mut Cx, value: Option<&str>) -> Value {
    match value {
        Some(value) => cx.factory().string(value.to_owned()).unwrap(),
        None => cx.factory().nil().unwrap(),
    }
}

fn temp_script(source: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sim-codec-lisp-cli-{}-{}.sim",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, source).unwrap();
    path
}
