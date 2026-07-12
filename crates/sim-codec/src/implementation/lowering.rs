//! Structure-preserving expression lowering helpers.

use sim_kernel::Expr;

/// Lower infix, prefix, and postfix operator nodes into explicit call nodes.
///
/// Container structure is preserved: list, vector, map, set, and block nodes
/// remain the same kind of node while their children are lowered recursively.
/// Quoted payloads are intentionally left unchanged so quotation remains an
/// opaque expression boundary.
pub fn lower_operator_nodes(expr: Expr) -> Expr {
    match expr {
        Expr::Infix {
            operator,
            left,
            right,
        } => Expr::Call {
            operator: Box::new(Expr::Symbol(operator)),
            args: vec![lower_operator_nodes(*left), lower_operator_nodes(*right)],
        },
        Expr::Prefix { operator, arg } | Expr::Postfix { operator, arg } => Expr::Call {
            operator: Box::new(Expr::Symbol(operator)),
            args: vec![lower_operator_nodes(*arg)],
        },
        Expr::Call { operator, args } => Expr::Call {
            operator: Box::new(lower_operator_nodes(*operator)),
            args: args.into_iter().map(lower_operator_nodes).collect(),
        },
        Expr::List(items) => Expr::List(items.into_iter().map(lower_operator_nodes).collect()),
        Expr::Vector(items) => Expr::Vector(items.into_iter().map(lower_operator_nodes).collect()),
        Expr::Map(entries) => Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| (lower_operator_nodes(key), lower_operator_nodes(value)))
                .collect(),
        ),
        Expr::Set(items) => Expr::Set(items.into_iter().map(lower_operator_nodes).collect()),
        Expr::Block(items) => Expr::Block(items.into_iter().map(lower_operator_nodes).collect()),
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(lower_operator_nodes(*expr)),
            annotations,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use sim_kernel::{Expr, QuoteMode, Symbol};

    use super::lower_operator_nodes;

    fn sym(name: &str) -> Symbol {
        Symbol::new(name)
    }

    fn sym_expr(name: &str) -> Expr {
        Expr::Symbol(sym(name))
    }

    fn infix(operator: &str, left: Expr, right: Expr) -> Expr {
        Expr::Infix {
            operator: sym(operator),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn prefix(operator: &str, arg: Expr) -> Expr {
        Expr::Prefix {
            operator: sym(operator),
            arg: Box::new(arg),
        }
    }

    fn postfix(operator: &str, arg: Expr) -> Expr {
        Expr::Postfix {
            operator: sym(operator),
            arg: Box::new(arg),
        }
    }

    fn call(operator: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            operator: Box::new(sym_expr(operator)),
            args,
        }
    }

    #[test]
    fn lowers_nested_operators_inside_calls() {
        let expr = Expr::Call {
            operator: Box::new(prefix("resolve", sym_expr("f"))),
            args: vec![
                infix("+", sym_expr("a"), postfix("!", sym_expr("b"))),
                prefix("-", infix("*", sym_expr("c"), sym_expr("d"))),
            ],
        };

        let expected = Expr::Call {
            operator: Box::new(call("resolve", vec![sym_expr("f")])),
            args: vec![
                call("+", vec![sym_expr("a"), call("!", vec![sym_expr("b")])]),
                call("-", vec![call("*", vec![sym_expr("c"), sym_expr("d")])]),
            ],
        };

        assert_eq!(lower_operator_nodes(expr), expected);
    }

    #[test]
    fn preserves_container_shapes_while_lowering_children() {
        let expr = Expr::Block(vec![
            Expr::List(vec![infix("+", sym_expr("a"), sym_expr("b"))]),
            Expr::Vector(vec![prefix("-", sym_expr("x"))]),
            Expr::Map(vec![(
                infix("*", sym_expr("k1"), sym_expr("k2")),
                postfix("?", sym_expr("v")),
            )]),
            Expr::Set(vec![infix("/", sym_expr("n"), sym_expr("d"))]),
        ]);

        let expected = Expr::Block(vec![
            Expr::List(vec![call("+", vec![sym_expr("a"), sym_expr("b")])]),
            Expr::Vector(vec![call("-", vec![sym_expr("x")])]),
            Expr::Map(vec![(
                call("*", vec![sym_expr("k1"), sym_expr("k2")]),
                call("?", vec![sym_expr("v")]),
            )]),
            Expr::Set(vec![call("/", vec![sym_expr("n"), sym_expr("d")])]),
        ]);

        assert_eq!(lower_operator_nodes(expr), expected);
    }

    #[test]
    fn leaves_quoted_operator_payloads_unchanged() {
        let quoted = Expr::Quote {
            mode: QuoteMode::Quote,
            expr: Box::new(infix("+", sym_expr("a"), sym_expr("b"))),
        };

        assert_eq!(lower_operator_nodes(quoted.clone()), quoted);
    }
}
