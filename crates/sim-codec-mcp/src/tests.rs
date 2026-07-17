use std::sync::Arc;

use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{DefaultFactory, EagerPolicy, EncodeOptions, Expr, NumberLiteral, Symbol};

use crate::{
    INVALID_PARAMS, McpCodecLib, McpEnvelope, McpError, McpErrorEnvelope, McpNotification,
    McpRequest, McpResponse, envelope_to_expr, expr_to_envelope,
};

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let lib = McpCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

fn codec_symbol() -> Symbol {
    Symbol::qualified("codec", "mcp")
}

fn number(value: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_owned(),
    })
}

fn int_number(value: i64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}

fn roundtrip(cx: &mut sim_kernel::Cx, expr: &Expr) -> Expr {
    let output = encode_with_codec(cx, &codec_symbol(), expr, EncodeOptions::default()).unwrap();
    let text = output.into_text().unwrap();
    assert!(!text.contains('\n'));
    decode_with_codec(cx, &codec_symbol(), Input::Text(text), Default::default()).unwrap()
}

#[test]
fn codec_registers() {
    let cx = cx();
    assert!(cx.registry().codec_by_symbol(&codec_symbol()).is_some());
}

#[test]
fn request_notification_response_and_error_roundtrip() {
    let mut cx = cx();
    let envelopes = vec![
        McpEnvelope::Request(McpRequest {
            id: Expr::String("req-1".to_owned()),
            method: "tools/list".to_owned(),
            params: Expr::Nil,
        }),
        McpEnvelope::Notification(McpNotification {
            method: "notifications/initialized".to_owned(),
            params: Expr::Map(vec![(Expr::Symbol(Symbol::new("ok")), Expr::Bool(true))]),
        }),
        McpEnvelope::Response(McpResponse {
            id: number("7"),
            result: Expr::String("ready".to_owned()),
        }),
        McpEnvelope::Error(McpErrorEnvelope {
            id: Expr::Nil,
            error: McpError {
                code: INVALID_PARAMS,
                message: "bad params".to_owned(),
                data: Expr::String("name is required".to_owned()),
            },
        }),
    ];

    for envelope in envelopes {
        let expr = envelope_to_expr(&envelope);
        assert_eq!(roundtrip(&mut cx, &expr), expr);
        assert_eq!(expr_to_envelope(&expr).unwrap(), envelope);
    }
}

#[test]
fn string_number_and_nil_ids_roundtrip() {
    let mut cx = cx();
    for id in [Expr::String("abc".to_owned()), number("42"), Expr::Nil] {
        let expr = envelope_to_expr(&McpEnvelope::Response(McpResponse {
            id,
            result: Expr::Nil,
        }));
        assert_eq!(roundtrip(&mut cx, &expr), expr);
    }
}

#[test]
fn decodes_wire_jsonrpc_request_to_canonical_expr_map() {
    let mut cx = cx();
    let decoded = decode_with_codec(
        &mut cx,
        &codec_symbol(),
        Input::Text(
            r#"{"jsonrpc":"2.0","id":"r1","method":"ping","params":{"$expr":"nil"}}"#.to_owned(),
        ),
        Default::default(),
    )
    .unwrap();

    assert_eq!(
        decoded,
        Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("mcp")),
                Expr::String("2.0".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("id")),
                Expr::String("r1".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("method")),
                Expr::String("ping".to_owned()),
            ),
            (Expr::Symbol(Symbol::new("params")), Expr::Nil),
        ])
    );
}

#[test]
fn invalid_inputs_fail_closed() {
    let mut cx = cx();
    for source in [
        r#"[{"jsonrpc":"2.0","method":"ping"}]"#,
        r#"{"jsonrpc":"2.0","method":"ping","extra":true}"#,
        r#"{"jsonrpc":"1.0","method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":true,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1,"result":{"not":"a tagged expr"}}"#,
    ] {
        assert!(
            decode_with_codec(
                &mut cx,
                &codec_symbol(),
                Input::Text(source.to_owned()),
                Default::default(),
            )
            .is_err(),
            "{source} should fail"
        );
    }
}

#[test]
fn duplicate_wire_envelope_fields_fail_closed() {
    let mut cx = cx();
    for source in [
        r#"{"jsonrpc":"2.0","jsonrpc":"2.0","method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":"r1","id":"r2","method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":"r1","method":"ping","method":"pong"}"#,
        r#"{"jsonrpc":"2.0","id":"r1","method":"ping","params":null,"params":null}"#,
        r#"{"jsonrpc":"2.0","id":"r1","result":null,"result":null}"#,
        r#"{"jsonrpc":"2.0","id":"r1","error":{"code":-1,"message":"x"},"error":{"code":-2,"message":"y"}}"#,
    ] {
        assert!(
            decode_with_codec(
                &mut cx,
                &codec_symbol(),
                Input::Text(source.to_owned()),
                Default::default(),
            )
            .is_err(),
            "{source} should fail"
        );
    }
}

#[test]
fn duplicate_wire_error_fields_fail_closed() {
    let mut cx = cx();
    for source in [
        r#"{"jsonrpc":"2.0","id":"r1","error":{"code":-1,"code":-2,"message":"x"}}"#,
        r#"{"jsonrpc":"2.0","id":"r1","error":{"code":-1,"message":"x","message":"y"}}"#,
        r#"{"jsonrpc":"2.0","id":"r1","error":{"code":-1,"message":"x","data":null,"data":null}}"#,
    ] {
        assert!(
            decode_with_codec(
                &mut cx,
                &codec_symbol(),
                Input::Text(source.to_owned()),
                Default::default(),
            )
            .is_err(),
            "{source} should fail"
        );
    }
}

#[test]
fn duplicate_payload_keys_are_not_mcp_envelope_fields() {
    let mut cx = cx();
    let decoded = decode_with_codec(
        &mut cx,
        &codec_symbol(),
        Input::Text(
            r#"{"jsonrpc":"2.0","id":"r1","method":"ping","params":{"$expr":"nil","$expr":"nil"}}"#
                .to_owned(),
        ),
        Default::default(),
    );

    assert!(decoded.is_ok());
}

#[test]
fn fuzz_style_invalid_jsonrpc_envelopes_fail_closed() {
    let mut cx = cx();
    let cases = [
        "",
        "null",
        "[]",
        r#"{"jsonrpc":"2.0"}"#,
        r#"{"jsonrpc":"2.0","id":"x"}"#,
        r#"{"jsonrpc":"2.0","id":"x","method":7}"#,
        r#"{"jsonrpc":"2.0","id":"x","method":"ping","result":null}"#,
        r#"{"jsonrpc":"2.0","id":"x","result":null,"error":{"code":-1,"message":"x"}}"#,
        r#"{"jsonrpc":"2.0","id":"x","error":{"code":"bad","message":"x"}}"#,
        r#"{"jsonrpc":"2.0","id":"x","error":{"code":-1,"message":7}}"#,
        r#"{"jsonrpc":"2.0","method":"ping","params":{"$expr":"unknown"}}"#,
    ];
    for source in cases {
        let result = decode_with_codec(
            &mut cx,
            &codec_symbol(),
            Input::Text(source.to_owned()),
            Default::default(),
        );
        assert!(result.is_err(), "{source:?} should fail closed");
    }
}

#[test]
fn non_envelope_exprs_do_not_encode() {
    let mut cx = cx();
    let invalid = Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("mcp")),
            Expr::String("2.0".to_owned()),
        ),
        (Expr::Symbol(Symbol::new("id")), number("1")),
        (Expr::Symbol(Symbol::new("result")), Expr::Nil),
        (Expr::Symbol(Symbol::new("extra")), Expr::Bool(true)),
    ]);

    assert!(
        encode_with_codec(&mut cx, &codec_symbol(), &invalid, EncodeOptions::default()).is_err()
    );
}

#[test]
fn error_code_expr_uses_integer_domain() {
    let expr = envelope_to_expr(&McpEnvelope::Error(McpErrorEnvelope {
        id: Expr::String("r1".to_owned()),
        error: McpError {
            code: INVALID_PARAMS,
            message: "bad params".to_owned(),
            data: Expr::Nil,
        },
    }));
    let Expr::Map(fields) = expr else {
        panic!("expected map");
    };
    let error = fields
        .iter()
        .find_map(|(key, value)| (key == &Expr::Symbol(Symbol::new("error"))).then_some(value))
        .unwrap();
    let Expr::Map(error_fields) = error else {
        panic!("expected error map");
    };
    assert_eq!(
        error_fields
            .iter()
            .find_map(|(key, value)| {
                (key == &Expr::Symbol(Symbol::new("code"))).then_some(value)
            })
            .unwrap(),
        &int_number(INVALID_PARAMS)
    );
}
