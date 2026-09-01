//! Cross-codec proof that platform contracts remain values, not host access.
// conformance: platform contracts round-trip across general-purpose codecs.

use sim_codec_algol::AlgolCodecLib;
use sim_codec_binary::BinaryCodecLib;
use sim_codec_bitwise::BitwiseCodecLib;
use sim_codec_json::JsonCodecLib;
use sim_codec_lisp::LispCodecLib;

use sim_codec::{Input, Output, decode_with_codec, encode_with_codec};
use sim_kernel::{Cx, EncodeOptions, Expr, ReadPolicy, Symbol};

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
    let mut cx = sim_kernel::testing::eager_cx();
    sim_test_support::register_core_classes(&mut cx);
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
