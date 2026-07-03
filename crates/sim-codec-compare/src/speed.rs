//! Dependency-free encode/decode timing harness (no `criterion`).
//!
//! Timings are machine-relative, so the report leans on RATIOS (bitwise/binary),
//! which are stable across machines, rather than absolute nanoseconds.

use std::time::{Duration, Instant};

use sim_kernel::{CodecId, Expr};

/// Encode + decode wall time per codec for one value (median of `reps`).
#[derive(Clone, Copy, Debug)]
pub struct Timings {
    /// Median `sim-codec-binary` encode time.
    pub binary_encode: Duration,
    /// Median `sim-codec-binary` decode time.
    pub binary_decode: Duration,
    /// Median `sim-codec-bitwise` encode time.
    pub bitwise_encode: Duration,
    /// Median `sim-codec-bitwise` decode time.
    pub bitwise_decode: Duration,
}

fn median_time(reps: u32, mut op: impl FnMut()) -> Duration {
    let warm = reps.clamp(1, 16);
    for _ in 0..warm {
        op();
    }
    let mut samples: Vec<Duration> = (0..reps.max(1))
        .map(|_| {
            let t = Instant::now();
            op();
            t.elapsed()
        })
        .collect();
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// Time all four operations for `expr` (median of `reps`).
pub fn measure_speed(expr: &Expr, reps: u32) -> Timings {
    let binary_bytes = sim_codec_binary::encode_frame(expr)
        .expect("binary encode")
        .0;
    let bitwise_bytes = sim_codec_bitwise::encode_frame(expr)
        .expect("bitwise encode")
        .0;

    Timings {
        binary_encode: median_time(reps, || {
            let _ = std::hint::black_box(sim_codec_binary::encode_frame(expr).unwrap());
        }),
        binary_decode: median_time(reps, || {
            let _ = std::hint::black_box(
                sim_codec_binary::decode_frame(CodecId(1), &binary_bytes).unwrap(),
            );
        }),
        bitwise_encode: median_time(reps, || {
            let _ = std::hint::black_box(sim_codec_bitwise::encode_frame(expr).unwrap());
        }),
        bitwise_decode: median_time(reps, || {
            let _ = std::hint::black_box(
                sim_codec_bitwise::decode_frame(CodecId(1), &bitwise_bytes).unwrap(),
            );
        }),
    }
}

fn ratio(slow: Duration, fast: Duration) -> f64 {
    if fast.as_nanos() == 0 {
        return f64::NAN;
    }
    slow.as_nanos() as f64 / fast.as_nanos() as f64
}

/// Mean bitwise/binary slowdown factor for encode and for decode across the corpus.
///
/// A factor > 1.0 means bitwise is slower than binary (the expected direction).
pub fn slowdown_factors(reps: u32) -> (f64, f64) {
    let samples = crate::corpus::corpus();
    let mut enc = 0.0;
    let mut dec = 0.0;
    for s in &samples {
        let t = measure_speed(&s.expr, reps);
        enc += ratio(t.bitwise_encode, t.binary_encode);
        dec += ratio(t.bitwise_decode, t.binary_decode);
    }
    let n = samples.len() as f64;
    (enc / n, dec / n)
}
