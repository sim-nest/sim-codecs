//! The comparison corpus: `Expr` values grouped by the payload shape they stress.

use sim_kernel::{Expr, NumberLiteral, Symbol};
use sim_value::build::sym;

/// What kind of payload a sample stresses -- the axis the verdict is sliced on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// A single small value: frame-overhead sensitivity.
    TinyScalar,
    /// A vector of small signed integers: signed minimal magnitude.
    SmallInts,
    /// A vector of large integers: magnitude vs fixed-width fields.
    BigInts,
    /// Non-integers: text fallback, no bit win expected.
    Floats,
    /// Interned symbol-heavy data: side-table dominated.
    Symbols,
    /// UTF-8 string blobs: raw bytes, no bit-packing headroom.
    Strings,
    /// Deeply nested lists: per-node tag + structure packing.
    DeepNested,
    /// Wide maps: canonical order + many keys.
    WideMap,
    /// A mixed record shaped like real runtime data.
    Realistic,
    /// Repeated identical subtrees: the dense-mode target.
    Repetitive,
}

/// All categories, in report order.
pub const CATEGORIES: [Category; 10] = [
    Category::TinyScalar,
    Category::SmallInts,
    Category::BigInts,
    Category::Floats,
    Category::Symbols,
    Category::Strings,
    Category::DeepNested,
    Category::WideMap,
    Category::Realistic,
    Category::Repetitive,
];

/// A stable name for a category (report rows).
pub fn category_name(c: Category) -> &'static str {
    match c {
        Category::TinyScalar => "TinyScalar",
        Category::SmallInts => "SmallInts",
        Category::BigInts => "BigInts",
        Category::Floats => "Floats",
        Category::Symbols => "Symbols",
        Category::Strings => "Strings",
        Category::DeepNested => "DeepNested",
        Category::WideMap => "WideMap",
        Category::Realistic => "Realistic",
        Category::Repetitive => "Repetitive",
    }
}

/// One corpus entry.
pub struct Sample {
    /// Human label for the report row.
    pub label: &'static str,
    /// The axis this sample stresses.
    pub category: Category,
    /// The value both codecs encode.
    pub expr: Expr,
}

fn num(domain: &str, canonical: &str) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", domain),
        canonical: canonical.to_owned(),
    })
}

fn int(n: i64) -> Expr {
    num("i64", &n.to_string())
}

fn deep(depth: usize) -> Expr {
    let mut e = int(0);
    for _ in 0..depth {
        e = Expr::List(vec![sym("node"), e]);
    }
    e
}

/// The full comparison corpus (>= 2 samples per category where useful).
pub fn corpus() -> Vec<Sample> {
    let mut v: Vec<Sample> = Vec::new();

    // TinyScalar -- one small value; frame overhead dominates.
    v.push(Sample {
        label: "int:0",
        category: Category::TinyScalar,
        expr: int(0),
    });
    v.push(Sample {
        label: "int:7",
        category: Category::TinyScalar,
        expr: int(7),
    });
    v.push(Sample {
        label: "bool",
        category: Category::TinyScalar,
        expr: Expr::Bool(true),
    });
    v.push(Sample {
        label: "sym:x",
        category: Category::TinyScalar,
        expr: sym("x"),
    });

    // SmallInts -- signed minimal magnitude should shine here.
    v.push(Sample {
        label: "ints:[-64..64)",
        category: Category::SmallInts,
        expr: Expr::List((-64..64).map(int).collect()),
    });
    v.push(Sample {
        label: "ints:[0..128)",
        category: Category::SmallInts,
        expr: Expr::List((0..128).map(int).collect()),
    });

    // BigInts -- large magnitudes; the win narrows.
    v.push(Sample {
        label: "ints:i64::MAX x64",
        category: Category::BigInts,
        expr: Expr::List((0..64).map(|_| int(i64::MAX)).collect()),
    });
    v.push(Sample {
        label: "ints:mixed-big",
        category: Category::BigInts,
        expr: Expr::List(
            [
                1_000_000_i64,
                -999_999_999,
                4_294_967_296,
                -8_589_934_592,
                123_456_789,
            ]
            .iter()
            .cycle()
            .take(64)
            .map(|n| int(*n))
            .collect(),
        ),
    });

    // Floats -- non-integers fall back to text; no bit win expected.
    v.push(Sample {
        label: "floats:pi-ish x64",
        category: Category::Floats,
        expr: Expr::List(
            [
                "3.14159265358979",
                "2.71828182845905",
                "1.41421356237310",
                "0.57721566490153",
            ]
            .iter()
            .cycle()
            .take(64)
            .map(|s| num("f64", s))
            .collect(),
        ),
    });

    // Symbols -- side-table dominated.
    v.push(Sample {
        label: "symbols x64",
        category: Category::Symbols,
        expr: Expr::List(
            [
                "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
            ]
            .iter()
            .cycle()
            .take(64)
            .map(|s| sym(s))
            .collect(),
        ),
    });

    // Strings -- raw UTF-8; the honest non-win.
    v.push(Sample {
        label: "strings x32",
        category: Category::Strings,
        expr: Expr::List(
            (0..32)
                .map(|i| Expr::String(format!("the quick brown fox jumps over lazy dog #{i}")))
                .collect(),
        ),
    });

    // DeepNested -- per-node tag + structure.
    v.push(Sample {
        label: "deep:32",
        category: Category::DeepNested,
        expr: deep(32),
    });
    v.push(Sample {
        label: "deep:64",
        category: Category::DeepNested,
        expr: deep(64),
    });

    // WideMap -- many keys, canonical order.
    v.push(Sample {
        label: "map:48 keys",
        category: Category::WideMap,
        expr: Expr::Map(
            (0..48)
                .map(|i| {
                    (
                        sym(match i % 4 {
                            0 => "id",
                            1 => "name",
                            2 => "count",
                            _ => "flag",
                        }),
                        int(i),
                    )
                })
                .collect(),
        ),
    });

    // Realistic -- a record shaped like real runtime data.
    v.push(Sample {
        label: "record:mixed",
        category: Category::Realistic,
        expr: Expr::Map(vec![
            (sym("id"), int(42)),
            (sym("name"), Expr::String("sim-runtime".to_owned())),
            (sym("enabled"), Expr::Bool(true)),
            (sym("scores"), Expr::Vector((0..16).map(int).collect())),
            (
                sym("tags"),
                Expr::Vector(vec![sym("a"), sym("b"), sym("c")]),
            ),
            (sym("note"), Expr::String("a short human note".to_owned())),
        ]),
    });

    // Repetitive -- repeated identical subtrees; the dense-mode target.
    let sub = Expr::Map(vec![
        (sym("k1"), int(100)),
        (sym("k2"), Expr::String("payload".to_owned())),
        (sym("k3"), Expr::Vector(vec![int(1), int(2), int(3)])),
    ]);
    v.push(Sample {
        label: "repeat:same-subtree x32",
        category: Category::Repetitive,
        expr: Expr::List((0..32).map(|_| sub.clone()).collect()),
    });

    v
}
