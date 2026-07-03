//! Shared test harness for SIM crates.
//!
//! The same `install_core_runtime` class-stub `Cx`, `roundtrip` codec helper,
//! and `contains_kind` scene search were copied into ~16 test modules. This
//! crate is their one home. It is used only as a `dev-dependency` and depends
//! only on `sim-kernel`, `sim-value`, and `sim-codec` (the codec API crate, not
//! any concrete codec lib), so it never forms a dependency cycle: a codec crate
//! constructs its own codec lib and passes it to [`load_then`], then calls
//! [`roundtrip`].

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::sync::Arc;

use sim_codec::{Input, Output, decode_with_codec, encode_with_codec};
use sim_kernel::{
    ClassId, Cx, DefaultFactory, EagerPolicy, EncodeOptions, Expr, Lib, ReadPolicy, Symbol,
};

pub use sim_value::build::{float, int, map, sym, text};

mod number_domain;

pub use number_domain::register_f64_number_domain;

/// The core class stubs a typical test runtime needs. A generous superset of
/// every copied `install_core_runtime`; registering extra stubs is harmless.
const CORE_CLASSES: &[(ClassId, &str)] = &[
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
];

/// Register the core class stubs into an existing `Cx` (preserving its policy
/// and factory).
pub fn register_core_classes(cx: &mut Cx) {
    for (id, name) in CORE_CLASSES {
        let symbol = Symbol::qualified("core", *name);
        let value = cx
            .factory()
            .class_stub(*id, symbol.clone())
            .expect("class stub");
        cx.registry_mut()
            .register_class_value(symbol, value)
            .expect("register class");
    }
}

/// A fresh `Cx` (eager policy, default factory) with the core class stubs
/// registered.
pub fn core_cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    register_core_classes(&mut cx);
    cx
}

/// Load a lib into `cx` and return its manifest id symbol.
pub fn load_then<L: Lib>(cx: &mut Cx, lib: L) -> Symbol {
    let id = lib.manifest().id.clone();
    cx.load_lib(&lib).expect("load lib");
    id
}

/// Conformance check for a host-registered card/backend lib: run `install`
/// twice (asserting idempotency), then assert the registered lib's manifest
/// exports cover every symbol in `expected`. This is the shared body of the many
/// `install_*_lib_registers_runtime_exports` tests.
pub fn assert_lib_exports(
    cx: &mut Cx,
    install: impl Fn(&mut Cx) -> sim_kernel::Result<()>,
    lib_id: &Symbol,
    expected: &[Symbol],
) {
    install(cx).expect("install");
    install(cx).expect("idempotent install");
    let manifest = &cx
        .registry()
        .lib(lib_id)
        .expect("lib should be registered")
        .manifest;
    for symbol in expected {
        assert!(
            manifest
                .exports
                .iter()
                .any(|export| export.symbol() == symbol),
            "missing export {symbol}"
        );
    }
}

/// Encode `expr` through the codec named `codec` (local name, e.g. `"lisp"`),
/// then decode the result -- the standard codec round-trip helper. The codec
/// must already be loaded into `cx`.
pub fn roundtrip(cx: &mut Cx, codec: &str, expr: &Expr) -> Expr {
    roundtrip_sym(cx, &Symbol::qualified("codec", codec), expr)
}

/// Like [`roundtrip`] but addressing the codec by its full `Symbol`.
pub fn roundtrip_sym(cx: &mut Cx, codec: &Symbol, expr: &Expr) -> Expr {
    let output =
        encode_with_codec(cx, codec, expr, EncodeOptions::default()).expect("encode_with_codec");
    let input = match output {
        Output::Text(text) => Input::Text(text),
        Output::Bytes(bytes) => Input::Bytes(bytes),
    };
    decode_with_codec(cx, codec, input, ReadPolicy::default()).expect("decode_with_codec")
}

/// Whether `scene` contains a scene node tagged with `kind` anywhere in its
/// tree.
pub fn contains_kind(scene: &Expr, kind: &str) -> bool {
    if node_kind_name(scene) == Some(kind) {
        return true;
    }
    match scene {
        Expr::Map(entries) => entries.iter().any(|(_, value)| contains_kind(value, kind)),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) => {
            items.iter().any(|item| contains_kind(item, kind))
        }
        _ => false,
    }
}

fn node_kind_name(scene: &Expr) -> Option<&str> {
    let Expr::Map(entries) = scene else {
        return None;
    };
    entries.iter().find_map(|(key, value)| {
        let is_kind =
            matches!(key, Expr::Symbol(symbol) if &*symbol.name == "kind" && symbol.namespace.is_none());
        match value {
            Expr::Symbol(symbol) if is_kind => Some(symbol.name.as_ref()),
            _ => None,
        }
    })
}
