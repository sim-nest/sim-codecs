//! Regression-guarding assertions that LOCK the BITWISE_4 findings.
//!
//! Thresholds are set from the measured 2026-07-01 report with a safety margin;
//! they encode the analysis so it cannot silently regress. Run the `report`
//! binary to see the live numbers.

use crate::corpus::{Category, corpus};
use crate::size::measure_size;

fn mean_ratio(cat: Category) -> f64 {
    let (mut b, mut w) = (0.0f64, 0.0f64);
    for s in corpus().into_iter().filter(|s| s.category == cat) {
        let z = measure_size(&s.expr);
        b += z.binary as f64;
        w += z.bitwise as f64;
    }
    w / b
}

fn mean_dense_ratio(cat: Category) -> f64 {
    let (mut b, mut d) = (0.0f64, 0.0f64);
    for s in corpus().into_iter().filter(|s| s.category == cat) {
        let z = measure_size(&s.expr);
        b += z.binary as f64;
        d += z.bitwise_dense as f64;
    }
    d / b
}

// WIN: signed minimal magnitude roughly halves integer-dense payloads (~0.50).
#[test]
fn bitwise_wins_big_on_integer_dense() {
    assert!(
        mean_ratio(Category::SmallInts) < 0.60,
        "SmallInts {}",
        mean_ratio(Category::SmallInts)
    );
    assert!(
        mean_ratio(Category::BigInts) < 0.60,
        "BigInts {}",
        mean_ratio(Category::BigInts)
    );
}

// WIN: realistic mixed runtime data is ~40% smaller (~0.61).
#[test]
fn bitwise_wins_on_realistic_and_structure() {
    assert!(
        mean_ratio(Category::Realistic) < 0.75,
        "Realistic {}",
        mean_ratio(Category::Realistic)
    );
    assert!(
        mean_ratio(Category::WideMap) < 0.70,
        "WideMap {}",
        mean_ratio(Category::WideMap)
    );
    assert!(
        mean_ratio(Category::DeepNested) < 0.70,
        "DeepNested {}",
        mean_ratio(Category::DeepNested)
    );
}

// NON-WIN, stated honestly: raw UTF-8 does not bit-pack; strings ~tie (~0.998),
// floats near-tie (~0.956). Bitwise must never be LARGER than binary, though.
#[test]
fn bitwise_does_not_win_on_strings_or_floats() {
    let strings = mean_ratio(Category::Strings);
    let floats = mean_ratio(Category::Floats);
    assert!((0.95..=1.0).contains(&strings), "Strings {strings}");
    assert!((0.90..=1.0).contains(&floats), "Floats {floats}");
}

// bitwise plain is never larger than binary anywhere in the corpus.
#[test]
fn bitwise_is_never_larger_than_binary() {
    for s in corpus() {
        let z = measure_size(&s.expr);
        assert!(
            z.bitwise <= z.binary,
            "{} bitwise {} > binary {}",
            s.label,
            z.bitwise,
            z.binary
        );
    }
}

// DENSE: transformative on repetition (Repetitive ~0.07 of binary), but it is NOT
// free -- the Ref table is overhead on non-repetitive data, so dense is a targeted
// tool, not a universal default.
#[test]
fn dense_is_transformative_on_repetition_only() {
    assert!(
        mean_dense_ratio(Category::Repetitive) < 0.20,
        "Repetitive dense {}",
        mean_dense_ratio(Category::Repetitive)
    );
    assert!(
        mean_dense_ratio(Category::BigInts) < 0.20,
        "BigInts dense {}",
        mean_dense_ratio(Category::BigInts)
    );
    // On non-repetitive structured data, dense carries overhead (exceeds plain).
    let wide_plain = mean_ratio(Category::WideMap);
    let wide_dense = mean_dense_ratio(Category::WideMap);
    assert!(
        wide_dense > wide_plain,
        "dense should carry overhead on WideMap: dense {wide_dense} plain {wide_plain}"
    );
}
