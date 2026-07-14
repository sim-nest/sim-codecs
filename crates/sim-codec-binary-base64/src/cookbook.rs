use sim_kernel::{Args, Callable, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value};

use crate::base64::{decode_base64_with_limits, encode_base64};

pub(crate) fn roundtrip_report_symbol() -> Symbol {
    Symbol::qualified("binary-base64", "roundtrip-report")
}

pub(crate) struct BinaryBase64RoundtripReport;

impl Callable for BinaryBase64RoundtripReport {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        if !args.values().is_empty() {
            return Err(Error::Eval(format!(
                "{} expects no arguments",
                roundtrip_report_symbol()
            )));
        }
        let sample = sample_expr();
        let frame = sim_codec_binary::encode_frame(&sample)?;
        let text = encode_base64(&frame.0);
        let bytes = decode_base64_with_limits(
            sim_kernel::CodecId(0),
            &text,
            sim_codec::DecodeLimits::default(),
        )?;
        let (_, decoded) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        let report = Expr::Map(vec![
            field(
                "kind",
                Expr::Symbol(Symbol::qualified("codec", "roundtrip")),
            ),
            field("codec", Expr::String("codec/binary-base64".to_owned())),
            field("wire", Expr::String("base64 text over SLB8".to_owned())),
            field("encoded-chars", Expr::String(text.len().to_string())),
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

impl Object for BinaryBase64RoundtripReport {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(roundtrip_report_symbol().to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for BinaryBase64RoundtripReport {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

fn sample_expr() -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::qualified("codec", "binary-base64-demo")),
        Expr::String("text wrapper".to_owned()),
        Expr::Bool(true),
    ])
}

fn field(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}
