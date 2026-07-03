//! Smoke test for the timing harness. Absolute speed is machine-dependent and
//! flaky, so this only asserts the harness produces finite, non-zero timings; the
//! speed VERDICT is reported (see the `report` binary), not gated.

use crate::corpus::corpus;
use crate::speed::{measure_speed, slowdown_factors};

#[test]
fn speed_harness_produces_timings() {
    for s in corpus() {
        let t = measure_speed(&s.expr, 32);
        assert!(
            t.binary_encode.as_nanos() > 0,
            "binary_encode zero for {}",
            s.label
        );
        assert!(
            t.bitwise_encode.as_nanos() > 0,
            "bitwise_encode zero for {}",
            s.label
        );
        assert!(
            t.binary_decode.as_nanos() > 0,
            "binary_decode zero for {}",
            s.label
        );
        assert!(
            t.bitwise_decode.as_nanos() > 0,
            "bitwise_decode zero for {}",
            s.label
        );
    }
}

#[test]
fn slowdown_factors_are_finite_and_positive() {
    let (enc, dec) = slowdown_factors(32);
    assert!(enc.is_finite() && enc > 0.0, "encode factor {enc}");
    assert!(dec.is_finite() && dec > 0.0, "decode factor {dec}");
}
