//! Shared chat content-part and token-usage builders.
//!
//! The chat message content-part shape `{type: text, text: <text>}` and the
//! `numbers/f64` token-usage entries were independently re-grown by every crate
//! that bridges a provider transcript (OpenAI server, Ollama codec, HTTP and
//! process runners). These builders are the one home for that shape so the
//! canonical spelling cannot drift.

use sim_kernel::{Expr, NumberLiteral, Symbol};

/// A chat text content part: the map `{type: text, text: <text>}`.
pub fn text_part(text: &str) -> Expr {
    Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("type")),
            Expr::Symbol(Symbol::new("text")),
        ),
        (
            Expr::Symbol(Symbol::new("text")),
            Expr::String(text.to_owned()),
        ),
    ])
}

/// A `numbers/f64`-domain map entry `(name, value)` for usage and metric
/// records.
pub fn number_field(name: &str, value: u64) -> (Expr, Expr) {
    (
        Expr::Symbol(Symbol::new(name)),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: value.to_string(),
        }),
    )
}

/// The canonical token-usage entries built from optional counts, in the fixed
/// order `input-tokens`, `output-tokens`, `total-tokens`. Each present count
/// becomes one [`number_field`]; absent counts are skipped. The caller decides
/// how to wrap the result, because providers differ: some emit a usage map even
/// when empty, others omit usage entirely when no counts are present.
pub fn usage_record(
    input: Option<u64>,
    output: Option<u64>,
    total: Option<u64>,
) -> Vec<(Expr, Expr)> {
    let mut fields = Vec::new();
    if let Some(value) = input {
        fields.push(number_field("input-tokens", value));
    }
    if let Some(value) = output {
        fields.push(number_field("output-tokens", value));
    }
    if let Some(value) = total {
        fields.push(number_field("total-tokens", value));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_part_shape_is_stable() {
        assert_eq!(
            text_part("hi"),
            Expr::Map(vec![
                (
                    Expr::Symbol(Symbol::new("type")),
                    Expr::Symbol(Symbol::new("text")),
                ),
                (
                    Expr::Symbol(Symbol::new("text")),
                    Expr::String("hi".to_owned()),
                ),
            ])
        );
    }

    #[test]
    fn number_field_uses_qualified_f64_domain() {
        let (key, value) = number_field("input-tokens", 7);
        assert_eq!(key, Expr::Symbol(Symbol::new("input-tokens")));
        assert_eq!(
            value,
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "7".to_owned(),
            })
        );
    }

    #[test]
    fn usage_record_skips_absent_counts_in_order() {
        let fields = usage_record(Some(3), None, Some(5));
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, Expr::Symbol(Symbol::new("input-tokens")));
        assert_eq!(fields[1].0, Expr::Symbol(Symbol::new("total-tokens")));
        assert!(usage_record(None, None, None).is_empty());
    }
}
