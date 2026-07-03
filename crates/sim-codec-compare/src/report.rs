//! Aggregate the per-sample measurements into per-category report tables.

use crate::corpus::{CATEGORIES, Category, category_name, corpus};
use crate::size::measure_size;
use crate::speed::measure_speed;

/// A per-category size summary: mean bytes and the bitwise/binary ratio.
pub struct SizeRow {
    /// The category summarized.
    pub category: Category,
    /// Mean `binary` bytes over the category's samples.
    pub binary: f64,
    /// Mean `bitwise` plain bytes.
    pub bitwise: f64,
    /// Mean `bitwise` dense bytes.
    pub dense: f64,
    /// `bitwise / binary` mean ratio (< 1.0 means bitwise wins on size).
    pub ratio: f64,
}

fn mean(xs: &[usize]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<usize>() as f64 / xs.len() as f64
}

/// Mean sizes per category and the bitwise/binary ratio.
pub fn size_rows() -> Vec<SizeRow> {
    let samples = corpus();
    let mut rows = Vec::new();
    for &cat in &CATEGORIES {
        let (mut b, mut w, mut d) = (Vec::new(), Vec::new(), Vec::new());
        for s in samples.iter().filter(|s| s.category == cat) {
            let z = measure_size(&s.expr);
            b.push(z.binary);
            w.push(z.bitwise);
            d.push(z.bitwise_dense);
        }
        if b.is_empty() {
            continue;
        }
        let (binary, bitwise, dense) = (mean(&b), mean(&w), mean(&d));
        rows.push(SizeRow {
            category: cat,
            binary,
            bitwise,
            dense,
            ratio: if binary > 0.0 {
                bitwise / binary
            } else {
                f64::NAN
            },
        });
    }
    rows
}

/// Render `size_rows()` as an ASCII markdown table for the report + README.
pub fn size_table() -> String {
    let mut out = String::from(
        "| category   | binary | bitwise | dense | bitwise/binary |\n\
         |------------|-------:|--------:|------:|---------------:|\n",
    );
    for r in size_rows() {
        out.push_str(&format!(
            "| {:<10} | {:6.0} | {:7.0} | {:5.0} | {:14.3} |\n",
            category_name(r.category),
            r.binary,
            r.bitwise,
            r.dense,
            r.ratio,
        ));
    }
    out
}

/// Render a per-category SPEED table (bitwise/binary slowdown, encode + decode).
pub fn speed_table(reps: u32) -> String {
    let samples = corpus();
    let mut out = String::from(
        "| category   | enc slowdown | dec slowdown |\n\
         |------------|-------------:|-------------:|\n",
    );
    for &cat in &CATEGORIES {
        let (mut enc, mut dec, mut n) = (0.0f64, 0.0f64, 0u32);
        for s in samples.iter().filter(|s| s.category == cat) {
            let t = measure_speed(&s.expr, reps);
            let be = t.binary_encode.as_nanos().max(1) as f64;
            let bd = t.binary_decode.as_nanos().max(1) as f64;
            enc += t.bitwise_encode.as_nanos() as f64 / be;
            dec += t.bitwise_decode.as_nanos() as f64 / bd;
            n += 1;
        }
        if n == 0 {
            continue;
        }
        out.push_str(&format!(
            "| {:<10} | {:11.2}x | {:11.2}x |\n",
            category_name(cat),
            enc / n as f64,
            dec / n as f64,
        ));
    }
    out
}
