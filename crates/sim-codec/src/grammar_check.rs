//! Grammar-check reports over decoded codec text.

use sim_kernel::{Cx, Diagnostic, Expr, ReadPolicy, Result, Symbol};
use sim_shape::Shape;

use crate::{DecodePosition, DecodedForm, Input, decode_default_with_codec};

/// Report returned by [`grammar_check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarCheck {
    /// Whether decoding and Shape checking both accepted the text.
    pub accepted: bool,
    /// The decoded form, absent when the codec rejected the text.
    pub decoded: Option<DecodedForm>,
    /// Diagnostics from the decode or Shape check.
    pub diagnostics: Vec<Diagnostic>,
}

/// Decode `text` with `codec`, then check the decoded syntax against `shape`.
///
/// Decode errors are reported as an unaccepted [`GrammarCheck`] instead of
/// escaping as errors, so callers can feed the report into a repair loop. Runtime
/// lookup errors and Shape implementation errors still return `Err`.
pub fn grammar_check(
    cx: &mut Cx,
    shape: &dyn Shape,
    codec: &Symbol,
    text: &str,
    position: DecodePosition,
) -> Result<GrammarCheck> {
    let decoded = match decode_default_with_codec(
        cx,
        codec,
        Input::Text(text.to_owned()),
        ReadPolicy::default(),
        position,
    ) {
        Ok(decoded) => decoded,
        Err(err) if is_decode_report(&err) => {
            return Ok(GrammarCheck {
                accepted: false,
                decoded: None,
                diagnostics: vec![Diagnostic::error(format!(
                    "decode with {codec} failed: {err}"
                ))],
            });
        }
        Err(err) => return Err(err),
    };
    let expr = decoded_expr(&decoded);
    let matched = shape.check_expr(cx, &expr)?;
    Ok(GrammarCheck {
        accepted: matched.accepted,
        decoded: Some(decoded),
        diagnostics: matched.diagnostics,
    })
}

fn decoded_expr(decoded: &DecodedForm) -> Expr {
    match decoded {
        DecodedForm::Datum(datum) => Expr::from(datum.clone()),
        DecodedForm::Term(term) => Expr::from(term.clone()),
    }
}

fn is_decode_report(error: &sim_kernel::Error) -> bool {
    matches!(
        error,
        sim_kernel::Error::CodecError { .. }
            | sim_kernel::Error::TypeMismatch { .. }
            | sim_kernel::Error::Eval(_)
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{
        AbiVersion, CodecId, Cx, Datum, DefaultFactory, Dependency, Export, Expr, Factory, Lib,
        LibManifest, LibTarget, Linker, LoadCx, MatchScore, Result, ShapeMatch, Symbol, Version,
        testing::eager_cx as cx,
    };

    use crate::{CodecDefaultDecode, CodecRuntime, Decoder, Input, ReadCx, codec_value};

    use super::*;

    #[test]
    fn grammar_check_accepts_decoded_shape_match() {
        let mut cx = cx();
        install_text_codec(&mut cx);
        let check = grammar_check(
            &mut cx,
            &StringOnlyShape,
            &codec_symbol(),
            "ok",
            DecodePosition::Data,
        )
        .unwrap();

        assert!(check.accepted);
        assert_eq!(
            check.decoded,
            Some(DecodedForm::Datum(Datum::String("ok".to_owned())))
        );
        assert!(check.diagnostics.is_empty());
    }

    #[test]
    fn grammar_check_reports_decode_failure_without_decoded_form() {
        let mut cx = cx();
        install_text_codec(&mut cx);
        let check = grammar_check(
            &mut cx,
            &StringOnlyShape,
            &codec_symbol(),
            "decode-error",
            DecodePosition::Data,
        )
        .unwrap();

        assert!(!check.accepted);
        assert!(check.decoded.is_none());
        assert!(
            check
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("decode with codec/test failed"))
        );
    }

    #[test]
    fn grammar_check_preserves_shape_diagnostics() {
        let mut cx = cx();
        install_text_codec(&mut cx);
        let check = grammar_check(
            &mut cx,
            &StringOnlyShape,
            &codec_symbol(),
            "not-ok",
            DecodePosition::Data,
        )
        .unwrap();

        assert!(!check.accepted);
        assert_eq!(
            check.decoded,
            Some(DecodedForm::Datum(Datum::String("not-ok".to_owned())))
        );
        assert_eq!(check.diagnostics.len(), 1);
        assert_eq!(check.diagnostics[0].message, "expected ok");
    }

    fn install_text_codec(cx: &mut Cx) {
        cx.load_lib(&TextCodecLib).unwrap();
    }

    fn codec_symbol() -> Symbol {
        Symbol::qualified("codec", "test")
    }

    struct StringOnlyShape;

    impl sim_kernel::Shape for StringOnlyShape {
        fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
            match expr {
                Expr::String(text) if text == "ok" => Ok(ShapeMatch::accept(MatchScore::exact(1))),
                Expr::String(_) => Ok(ShapeMatch::reject("expected ok")),
                _ => Ok(ShapeMatch::reject("expected string")),
            }
        }

        fn check_value(&self, cx: &mut Cx, value: sim_kernel::Value) -> Result<ShapeMatch> {
            let expr = value.object().as_expr(cx)?;
            self.check_expr(cx, &expr)
        }

        fn describe(&self, _cx: &mut Cx) -> Result<sim_kernel::ShapeDoc> {
            Ok(sim_kernel::ShapeDoc::new("ok string"))
        }
    }

    struct TextDecoder;

    impl Decoder for TextDecoder {
        fn decode(&self, _cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
            let text = input.into_string()?;
            if text == "decode-error" {
                return Err(sim_kernel::Error::CodecError {
                    codec: CodecId(1),
                    message: "fixture decode failure".to_owned(),
                });
            }
            Ok(Expr::String(text))
        }
    }

    struct TextCodecLib;

    impl Lib for TextCodecLib {
        fn manifest(&self) -> LibManifest {
            LibManifest {
                id: codec_symbol(),
                version: Version("0.1.0".to_owned()),
                abi: AbiVersion { major: 0, minor: 1 },
                target: LibTarget::HostRegistered,
                requires: Vec::<Dependency>::new(),
                capabilities: Vec::new(),
                exports: vec![Export::Codec {
                    symbol: codec_symbol(),
                    codec_id: Some(CodecId(1)),
                }],
            }
        }

        fn load(&self, _cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
            let nil = DefaultFactory.nil()?;
            linker.codec_value(
                codec_symbol(),
                codec_value(CodecRuntime {
                    id: CodecId(1),
                    symbol: codec_symbol(),
                    decoder: Some(Arc::new(TextDecoder)),
                    located_decoder: None,
                    tree_decoder: None,
                    encoder: None,
                    located_encoder: None,
                    tree_encoder: None,
                    expr_shape: nil.clone(),
                    options_shape: nil,
                    default_decode: CodecDefaultDecode::Datum,
                }),
            )?;
            Ok(())
        }
    }
}
