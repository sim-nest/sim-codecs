//! Cross-codec proof that platform contracts remain values, not host access.

use sim_codec_algol::AlgolCodecLib;
use sim_codec_binary::BinaryCodecLib;
use sim_codec_bitwise::BitwiseCodecLib;
use sim_codec_json::JsonCodecLib;
use sim_codec_lisp::LispCodecLib;
use std::sync::Arc;

use sim_codec::{Input, Output, decode_with_codec, encode_with_codec};
use sim_kernel::{
    Cx, DefaultFactory, EagerPolicy, EncodeOptions, Expr, HandleSeed, ReadPolicy, Symbol,
};

fn cx() -> Cx {
    let mut cx = Cx::new(
        Arc::new(EagerPolicy),
        Arc::new(DefaultFactory),
        HandleSeed::new(1),
    );
    for (id, name) in [
        (sim_kernel::CORE_CLASS_CLASS_ID, "Class"),
        (sim_kernel::CORE_CODEC_CLASS_ID, "Codec"),
        (sim_kernel::CORE_NUMBER_CLASS_ID, "Number"),
        (sim_kernel::CORE_SYMBOL_CLASS_ID, "Symbol"),
        (sim_kernel::CORE_STRING_CLASS_ID, "String"),
        (sim_kernel::CORE_EXPR_CLASS_ID, "Expr"),
        (sim_kernel::CORE_SHAPE_CLASS_ID, "Shape"),
        (sim_kernel::CORE_BOOL_CLASS_ID, "Bool"),
        (sim_kernel::CORE_LIST_CLASS_ID, "List"),
        (sim_kernel::CORE_BYTES_CLASS_ID, "Bytes"),
        (sim_kernel::CORE_TABLE_CLASS_ID, "Table"),
        (sim_kernel::CORE_FUNCTION_CLASS_ID, "Function"),
        (sim_kernel::CORE_CARD_CLASS_ID, "Card"),
        (sim_kernel::CORE_NUMBER_DOMAIN_CLASS_ID, "NumberDomain"),
    ] {
        let symbol = Symbol::qualified("core", name);
        let value = cx.factory().class_stub(id, symbol.clone()).unwrap();
        cx.registry_mut()
            .register_class_value(symbol, value)
            .unwrap();
    }
    cx
}

fn roundtrip(cx: &mut Cx, codec: &str, expr: &Expr) -> Expr {
    let symbol = Symbol::qualified("codec", codec);
    let encoded = encode_with_codec(cx, &symbol, expr, EncodeOptions::default()).unwrap();
    let input = match encoded {
        Output::Text(text) => Input::Text(text),
        Output::Bytes(bytes) => Input::Bytes(bytes),
    };
    decode_with_codec(cx, &symbol, input, ReadPolicy::default()).unwrap()
}

fn platform_record() -> Expr {
    Expr::Map(vec![
        (
            Expr::Symbol(Symbol::qualified("platform", "contract")),
            Expr::Symbol(Symbol::qualified("loader", "wasm-v1")),
        ),
        (
            Expr::Symbol(Symbol::new("artifact")),
            Expr::Bytes(vec![0, 97, 115, 109]),
        ),
        (
            Expr::Symbol(Symbol::new("mount")),
            Expr::Symbol(Symbol::qualified("mount", "model-modules")),
        ),
        (
            Expr::Symbol(Symbol::new("audio")),
            Expr::List(vec![
                Expr::String("48000".to_owned()),
                Expr::String("stereo".to_owned()),
                Expr::Bool(true),
            ]),
        ),
    ])
}

#[test]
fn every_general_codec_roundtrips_platform_and_domain_values() {
    let mut cx = cx();
    let binary = BinaryCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&binary).unwrap();
    let bitwise = BitwiseCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&bitwise).unwrap();
    let json = JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&json).unwrap();
    let lisp = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    let algol = AlgolCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&algol).unwrap();

    let record = platform_record();
    for codec in ["binary", "bitwise", "json", "lisp", "algol"] {
        assert_eq!(roundtrip(&mut cx, codec, &record), record);
    }
}
