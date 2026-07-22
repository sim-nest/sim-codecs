use sim_codec::{CodecRuntime, Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Expr, NumberLiteral, ReadPolicy, Symbol};

use crate::{ConfigCodecLib, ConfigDecoder, ConfigEncoder};

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_test_support::core_cx();
    let lib = ConfigCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

fn key(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name.to_owned()))
}

fn text(value: &str) -> Expr {
    Expr::String(value.to_owned())
}

fn int(value: i64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}

fn field<'a>(expr: &'a Expr, name: &str) -> &'a Expr {
    let Expr::Map(entries) = expr else {
        panic!("expected map");
    };
    entries
        .iter()
        .find_map(|(key, value)| (key == &self::key(name)).then_some(value))
        .unwrap_or_else(|| panic!("missing key {name:?}"))
}

fn codec_id(cx: &mut sim_kernel::Cx, symbol: &Symbol) -> sim_kernel::CodecId {
    cx.resolve_codec(symbol)
        .unwrap()
        .object()
        .as_any()
        .downcast_ref::<CodecRuntime>()
        .unwrap()
        .id
}

fn assert_codec_error(err: sim_kernel::Error, expected: sim_kernel::CodecId, needle: &str) {
    match err {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, expected);
            assert_ne!(codec, sim_kernel::CodecId(0));
            assert!(message.contains(needle), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn codec_registers() {
    let cx = cx();
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "config"))
            .is_some()
    );
}

#[test]
fn invalid_utf8_input_reports_config_codec_id() {
    let mut cx = cx();
    let symbol = Symbol::qualified("codec", "config");
    let expected = codec_id(&mut cx, &symbol);

    let err = decode_with_codec(
        &mut cx,
        &symbol,
        Input::Bytes(vec![0xff]),
        ReadPolicy::default(),
    )
    .unwrap_err();

    assert_codec_error(err, expected, "not valid UTF-8");
}

#[test]
fn decodes_per_library_config_as_table() {
    let decoded = ConfigDecoder::table()
        .decode_text(
            r#"
enabled = true
minimum_loaded = ["codec/lisp", "codec/json"]

[[loadable_lib]]
id = "numbers/cas"
source = "symbol:numbers/cas"
"#,
        )
        .unwrap();

    assert_eq!(field(&decoded, "enabled"), &Expr::Bool(true));
    assert_eq!(
        field(&decoded, "minimum_loaded"),
        &Expr::List(vec![text("codec/lisp"), text("codec/json")])
    );
    let Expr::List(loadable) = field(&decoded, "loadable_lib") else {
        panic!("expected repeated table list");
    };
    assert_eq!(loadable.len(), 1);
    assert_eq!(field(&loadable[0], "id"), &text("numbers/cas"));
}

#[test]
fn decodes_single_file_config_as_directory() {
    let decoded = ConfigDecoder::dir()
        .decode_text(
            r#"
[sim/cookbook]
minimum_loaded = ["codec/lisp"]

[[sim/cookbook.loadable_lib]]
id = "sim-codec-json"
source = "symbol:codec/json"

[stream/host]
sample_rate_hz = 48000
"#,
        )
        .unwrap();

    let cookbook = field(&decoded, "sim/cookbook");
    assert_eq!(
        field(cookbook, "minimum_loaded"),
        &Expr::List(vec![text("codec/lisp")])
    );
    let Expr::List(loadable) = field(cookbook, "loadable_lib") else {
        panic!("expected repeated loadable_lib table");
    };
    assert_eq!(field(&loadable[0], "source"), &text("symbol:codec/json"));
    assert_eq!(
        field(field(&decoded, "stream/host"), "sample_rate_hz"),
        &int(48000)
    );
}

#[test]
fn arrays_decode_scalars() {
    let decoded = ConfigDecoder::table()
        .decode_text(r#"names = ["codec/lisp", "codec/json", "codec/config"]"#)
        .unwrap();
    assert_eq!(
        field(&decoded, "names"),
        &Expr::List(vec![
            text("codec/lisp"),
            text("codec/json"),
            text("codec/config"),
        ])
    );
}

#[test]
fn repeated_tables_append_in_order() {
    let decoded = ConfigDecoder::table()
        .decode_text(
            r#"
[[loadable_lib]]
id = "first"

[[loadable_lib]]
id = "second"
"#,
        )
        .unwrap();
    let Expr::List(items) = field(&decoded, "loadable_lib") else {
        panic!("expected repeated table list");
    };
    assert_eq!(field(&items[0], "id"), &text("first"));
    assert_eq!(field(&items[1], "id"), &text("second"));
}

#[test]
fn table_roundtrips_through_canonical_text() {
    let expr = Expr::Map(vec![
        (key("enabled"), Expr::Bool(true)),
        (key("count"), int(2)),
        (key("empty"), Expr::List(Vec::new())),
        (
            key("loadable_lib"),
            Expr::List(vec![Expr::Map(vec![
                (key("id"), text("codec/config")),
                (key("source"), text("symbol:codec/config")),
            ])]),
        ),
    ]);

    let text = ConfigEncoder::new().encode_text(&expr).unwrap();
    let decoded = ConfigDecoder::table().decode_text(&text).unwrap();
    assert_eq!(decoded, expr);
}

#[test]
fn runtime_auto_decodes_table_and_dir() {
    let mut cx = cx();
    let codec = Symbol::qualified("codec", "config");
    let table = decode_with_codec(
        &mut cx,
        &codec,
        Input::Text("enabled = true".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(field(&table, "enabled"), &Expr::Bool(true));

    let dir = decode_with_codec(
        &mut cx,
        &codec,
        Input::Text("[codec/config]\nenabled = true\n".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(
        field(field(&dir, "codec/config"), "enabled"),
        &Expr::Bool(true)
    );
}

#[test]
fn runtime_default_decode_keeps_config_eval_text_inert() {
    let mut cx = cx();
    let codec = Symbol::qualified("codec", "config");
    let eval_text = "#(config/eval v1 :codec codec/lisp :source nil)";

    let decoded = decode_with_codec(
        &mut cx,
        &codec,
        Input::Text(format!("eval_node = \"{eval_text}\"\n")),
        ReadPolicy::default(),
    )
    .unwrap();

    assert_eq!(field(&decoded, "eval_node"), &text(eval_text));
}

#[test]
fn runtime_encodes_maps() {
    let mut cx = cx();
    let codec = Symbol::qualified("codec", "config");
    let output = encode_with_codec(
        &mut cx,
        &codec,
        &Expr::Map(vec![(key("enabled"), Expr::Bool(true))]),
        EncodeOptions::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(output, "enabled = true\n");
}

#[test]
fn rejects_non_ascii_input() {
    let err = ConfigDecoder::table()
        .decode_text("name = \"cafe\u{e9}\"")
        .unwrap_err();
    assert!(err.contains("ASCII"));
}

#[test]
fn rejects_malformed_section() {
    let err = ConfigDecoder::dir()
        .decode_text("[sim/cookbook\nvalue = true")
        .unwrap_err();
    assert!(err.contains("malformed section"));
}
