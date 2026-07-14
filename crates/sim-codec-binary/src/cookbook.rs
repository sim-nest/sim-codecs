use sim_kernel::{Args, Callable, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value};

use crate::{decode_frame, encode_frame};

pub(crate) fn roundtrip_report_symbol() -> Symbol {
    Symbol::qualified("binary", "roundtrip-report")
}

pub(crate) struct BinaryRoundtripReport;

impl Callable for BinaryRoundtripReport {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        if !args.values().is_empty() {
            return Err(Error::Eval(format!(
                "{} expects no arguments",
                roundtrip_report_symbol()
            )));
        }
        let sample = sample_expr();
        let frame = encode_frame(&sample)?;
        let (tables, decoded) = decode_frame(sim_kernel::CodecId(0), &frame.0)?;
        let report = Expr::Map(vec![
            field(
                "kind",
                Expr::Symbol(Symbol::qualified("codec", "roundtrip")),
            ),
            field("codec", Expr::String("codec/binary".to_owned())),
            field("wire", Expr::String("SLB8 bytes".to_owned())),
            field("encoded-bytes", Expr::String(frame.0.len().to_string())),
            field(
                "symbol-table",
                Expr::String(tables.symbols.len().to_string()),
            ),
            field("decoded", decoded.clone()),
            field("roundtrip", Expr::Bool(decoded.canonical_eq(&sample))),
            field(
                "lanes",
                Expr::List(vec![
                    Expr::String("encode".to_owned()),
                    Expr::String("decode".to_owned()),
                ]),
            ),
        ]);
        cx.factory().expr(report)
    }
}

impl Object for BinaryRoundtripReport {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(roundtrip_report_symbol().to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for BinaryRoundtripReport {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

fn sample_expr() -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::qualified("codec", "binary-demo")),
        Expr::String("byte frame".to_owned()),
        Expr::Bool(true),
    ])
}

fn field(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}
