use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, Symbol};
use sim_shape::{ExprKind, ExprKindShape, Shape, TableExtraPolicy, TableFieldSpec, TableShape};

use super::domain_form::parse_domain_form;

#[test]
fn projects_domain_values_to_expr_tree() {
    let form = parse_domain_form(
        "#(Chord 60 \"lead\" pitches=[60,\"64\",#(Note dur=1/4)] inner=#(Rest dur=1/8))",
    )
    .expect("parse");

    assert_eq!(
        form.to_expr_map(),
        Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("form")),
                Expr::String("Chord".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("args")),
                Expr::List(vec![
                    Expr::String("60".to_owned()),
                    Expr::String("lead".to_owned()),
                ]),
            ),
            (
                Expr::Symbol(Symbol::new("pitches")),
                Expr::List(vec![
                    Expr::String("60".to_owned()),
                    Expr::String("64".to_owned()),
                    Expr::Map(vec![
                        (
                            Expr::Symbol(Symbol::new("form")),
                            Expr::String("Note".to_owned()),
                        ),
                        (
                            Expr::Symbol(Symbol::new("dur")),
                            Expr::String("1/4".to_owned()),
                        ),
                    ]),
                ]),
            ),
            (
                Expr::Symbol(Symbol::new("inner")),
                Expr::Map(vec![
                    (
                        Expr::Symbol(Symbol::new("form")),
                        Expr::String("Rest".to_owned()),
                    ),
                    (
                        Expr::Symbol(Symbol::new("dur")),
                        Expr::String("1/8".to_owned()),
                    ),
                ]),
            ),
        ])
    );
}

#[test]
fn projected_domain_form_is_accepted_by_table_shape() {
    let form = parse_domain_form("#(Note dur=1/4)").expect("parse");
    let expr = form.to_expr_map();
    let string_shape = || Arc::new(ExprKindShape::new(ExprKind::String));
    let shape = TableShape::new(
        vec![
            TableFieldSpec {
                key: Symbol::new("form"),
                shape: string_shape(),
                required: true,
            },
            TableFieldSpec {
                key: Symbol::new("dur"),
                shape: string_shape(),
                required: true,
            },
        ],
        TableExtraPolicy::Reject,
    );
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

    assert!(shape.check_expr(&mut cx, &expr).unwrap().accepted);
}
