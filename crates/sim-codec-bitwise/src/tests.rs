//! Unit tests for the bitwise codec.
//!
//! The suite is split by frame area into sibling submodules so no single test
//! file grows unbounded: `frame_basics` (bit IO, vbits, tags, header),
//! `expr_numbers` (plain `Expr` round-trips and signed number encoding),
//! `origin_canonical` (located/tree origin roles and content-addressing), and
//! `dense` (structural-sharing mode plus registration/fail-closed). The shared
//! fixture helpers live here in the parent module and are used across the
//! submodules.

use std::sync::Arc;

use sim_kernel::{Args, DefaultFactory, EagerPolicy, Expr, NumberLiteral, Symbol};
use sim_value::access::{field as map_field, field_str as field_string};

use crate::bitio::BitReader;
use crate::{BitwiseCodecLib, DecodeLimits};

mod dense;
mod expr_numbers;
mod frame_basics;
mod origin_canonical;

// ---- shared helpers -------------------------------------------------------

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let lib = BitwiseCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

fn bit_length(value: u128) -> usize {
    (u128::BITS - value.leading_zeros()) as usize
}

fn reader(bytes: &[u8]) -> BitReader<'_> {
    BitReader::new(sim_kernel::CodecId(1), bytes, DecodeLimits::default()).unwrap()
}

fn num(domain: &str, canonical: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", domain),
        canonical: canonical.to_owned(),
    })
}

#[test]
fn roundtrip_report_function_runs() {
    let mut cx = cx();
    let report = call_report(&mut cx, Symbol::qualified("bitwise", "roundtrip-report"));
    assert_eq!(field_bool(&report, "roundtrip"), Some(true));
    assert_eq!(field_string(&report, "codec"), Some("codec/bitwise"));
}

fn call_report(cx: &mut sim_kernel::Cx, symbol: Symbol) -> Expr {
    let value = cx.registry().function_by_symbol(&symbol).unwrap().clone();
    let callable = value.object().as_callable().unwrap();
    let value = callable.call(cx, Args::new(Vec::new())).unwrap();
    value.object().as_expr(cx).unwrap()
}

fn field_bool(expr: &Expr, name: &str) -> Option<bool> {
    map_field(expr, name).and_then(|value| match value {
        Expr::Bool(value) => Some(*value),
        _ => None,
    })
}
