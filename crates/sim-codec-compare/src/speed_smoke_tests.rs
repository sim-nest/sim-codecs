//! Smoke test for the timing harness. Absolute speed is machine-dependent and
//! flaky, so this only asserts the harness produces finite, non-zero timings; the
//! speed VERDICT is reported (see the `report` binary), not gated.

use crate::corpus::corpus;
use std::time::Duration;

use crate::speed::{BenchmarkClock, measure_speed, slowdown_factors};

struct ModelClock(u64);

impl BenchmarkClock for ModelClock {
    fn measure(&mut self, operation: &mut dyn FnMut()) -> Duration {
        operation();
        self.0 += 1;
        Duration::from_nanos(self.0)
    }
}

#[test]
fn speed_harness_produces_timings() {
    for s in corpus() {
        let t = measure_speed(&s.expr, 32, &mut ModelClock(0));
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
    let (enc, dec) = slowdown_factors(32, &mut ModelClock(0));
    assert!(enc.is_finite() && enc > 0.0, "encode factor {enc}");
    assert!(dec.is_finite() && dec > 0.0, "decode factor {dec}");
}
