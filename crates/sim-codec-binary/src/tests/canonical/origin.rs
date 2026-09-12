use super::*;

fn assert_origin(tree: &LocatedExprTree, source: &str, start: usize) {
    let origin = tree.origin.as_ref().expect("origin must survive encoding");
    assert_eq!(origin.source.0, source);
    assert_eq!(
        origin.span,
        Span {
            start,
            end: start + 1
        }
    );
}

fn assert_node(tree: &LocatedExprTree, expr: &Expr, source: &str, start: usize) {
    assert_eq!(&tree.expr, expr);
    assert_origin(tree, source, start);
}

#[test]
fn tree_writer_keeps_reordered_set_items_with_their_origins() {
    let tree = LocatedExprTree {
        expr: Expr::Set(vec![
            Expr::String("z".to_owned()),
            Expr::String("a".to_owned()),
        ]),
        origin: None,
        children: vec![
            LocatedExprTree::without_children(
                Expr::String("z".to_owned()),
                Some(sample_origin("z.sim", 10)),
            ),
            LocatedExprTree::without_children(
                Expr::String("a".to_owned()),
                Some(sample_origin("a.sim", 20)),
            ),
        ],
    };

    let BinaryFrame(bytes) = encode_located_tree_frame(&tree, true).unwrap();
    let (_, decoded) = strict_decode(BinaryFrameLane::LocatedTree, &bytes).unwrap();
    assert_eq!(
        decoded.expr,
        Expr::Set(vec![
            Expr::String("a".to_owned()),
            Expr::String("z".to_owned())
        ])
    );
    assert_eq!(decoded.children[0].expr, Expr::String("a".to_owned()));
    assert_origin(&decoded.children[0], "a.sim", 20);
    assert_eq!(decoded.children[1].expr, Expr::String("z".to_owned()));
    assert_origin(&decoded.children[1], "z.sim", 10);
    assert_eq!(encode_located_tree_frame(&decoded, true).unwrap().0, bytes);
}

#[test]
fn tree_writer_keeps_reordered_map_entries_with_their_origins() {
    let tree = LocatedExprTree {
        expr: Expr::Map(vec![
            (Expr::String("z".to_owned()), Expr::Bool(false)),
            (Expr::String("a".to_owned()), Expr::Bool(true)),
        ]),
        origin: None,
        children: vec![
            LocatedExprTree::without_children(
                Expr::String("z".to_owned()),
                Some(sample_origin("key-z.sim", 10)),
            ),
            LocatedExprTree::without_children(
                Expr::Bool(false),
                Some(sample_origin("value-z.sim", 11)),
            ),
            LocatedExprTree::without_children(
                Expr::String("a".to_owned()),
                Some(sample_origin("key-a.sim", 20)),
            ),
            LocatedExprTree::without_children(
                Expr::Bool(true),
                Some(sample_origin("value-a.sim", 21)),
            ),
        ],
    };

    let BinaryFrame(bytes) = encode_located_tree_frame(&tree, true).unwrap();
    let (_, decoded) = strict_decode(BinaryFrameLane::LocatedTree, &bytes).unwrap();
    assert_eq!(decoded.children[0].expr, Expr::String("a".to_owned()));
    assert_origin(&decoded.children[0], "key-a.sim", 20);
    assert_eq!(decoded.children[1].expr, Expr::Bool(true));
    assert_origin(&decoded.children[1], "value-a.sim", 21);
    assert_eq!(decoded.children[2].expr, Expr::String("z".to_owned()));
    assert_origin(&decoded.children[2], "key-z.sim", 10);
    assert_eq!(decoded.children[3].expr, Expr::Bool(false));
    assert_origin(&decoded.children[3], "value-z.sim", 11);
    assert_eq!(encode_located_tree_frame(&decoded, true).unwrap().0, bytes);
}

#[test]
fn tree_writer_keeps_recursive_unordered_expressions_with_complete_origin_subtrees() {
    let set_expr = Expr::Set(vec![
        Expr::List(vec![Expr::String("z".to_owned())]),
        Expr::List(vec![Expr::String("a".to_owned())]),
    ]);
    let map_expr = Expr::Map(vec![
        (
            Expr::String("z".to_owned()),
            Expr::List(vec![Expr::Bool(false)]),
        ),
        (
            Expr::String("a".to_owned()),
            Expr::List(vec![Expr::Bool(true)]),
        ),
    ]);
    let tree = LocatedExprTree {
        expr: Expr::List(vec![set_expr.clone(), map_expr.clone()]),
        origin: Some(sample_origin("outer.sim", 0)),
        children: vec![
            LocatedExprTree {
                expr: set_expr,
                origin: Some(sample_origin("set.sim", 10)),
                children: vec![
                    LocatedExprTree {
                        expr: Expr::List(vec![Expr::String("z".to_owned())]),
                        origin: Some(sample_origin("set-z-item.sim", 11)),
                        children: vec![LocatedExprTree::without_children(
                            Expr::String("z".to_owned()),
                            Some(sample_origin("set-z-descendant.sim", 12)),
                        )],
                    },
                    LocatedExprTree {
                        expr: Expr::List(vec![Expr::String("a".to_owned())]),
                        origin: Some(sample_origin("set-a-item.sim", 13)),
                        children: vec![LocatedExprTree::without_children(
                            Expr::String("a".to_owned()),
                            Some(sample_origin("set-a-descendant.sim", 14)),
                        )],
                    },
                ],
            },
            LocatedExprTree {
                expr: map_expr,
                origin: Some(sample_origin("map.sim", 20)),
                children: vec![
                    LocatedExprTree::without_children(
                        Expr::String("z".to_owned()),
                        Some(sample_origin("map-z-key.sim", 21)),
                    ),
                    LocatedExprTree {
                        expr: Expr::List(vec![Expr::Bool(false)]),
                        origin: Some(sample_origin("map-z-value.sim", 22)),
                        children: vec![LocatedExprTree::without_children(
                            Expr::Bool(false),
                            Some(sample_origin("map-z-descendant.sim", 23)),
                        )],
                    },
                    LocatedExprTree::without_children(
                        Expr::String("a".to_owned()),
                        Some(sample_origin("map-a-key.sim", 24)),
                    ),
                    LocatedExprTree {
                        expr: Expr::List(vec![Expr::Bool(true)]),
                        origin: Some(sample_origin("map-a-value.sim", 25)),
                        children: vec![LocatedExprTree::without_children(
                            Expr::Bool(true),
                            Some(sample_origin("map-a-descendant.sim", 26)),
                        )],
                    },
                ],
            },
        ],
    };

    let BinaryFrame(bytes) = encode_located_tree_frame(&tree, true).unwrap();
    let (_, decoded) = strict_decode(BinaryFrameLane::LocatedTree, &bytes).unwrap();

    let canonical_set_expr = Expr::Set(vec![
        Expr::List(vec![Expr::String("a".to_owned())]),
        Expr::List(vec![Expr::String("z".to_owned())]),
    ]);
    let canonical_map_expr = Expr::Map(vec![
        (
            Expr::String("a".to_owned()),
            Expr::List(vec![Expr::Bool(true)]),
        ),
        (
            Expr::String("z".to_owned()),
            Expr::List(vec![Expr::Bool(false)]),
        ),
    ]);
    assert_node(
        &decoded,
        &Expr::List(vec![canonical_set_expr.clone(), canonical_map_expr.clone()]),
        "outer.sim",
        0,
    );
    let set = &decoded.children[0];
    assert_node(set, &canonical_set_expr, "set.sim", 10);
    assert_node(
        &set.children[0],
        &Expr::List(vec![Expr::String("a".to_owned())]),
        "set-a-item.sim",
        13,
    );
    assert_node(
        &set.children[0].children[0],
        &Expr::String("a".to_owned()),
        "set-a-descendant.sim",
        14,
    );
    assert_node(
        &set.children[1],
        &Expr::List(vec![Expr::String("z".to_owned())]),
        "set-z-item.sim",
        11,
    );
    assert_node(
        &set.children[1].children[0],
        &Expr::String("z".to_owned()),
        "set-z-descendant.sim",
        12,
    );

    let map = &decoded.children[1];
    assert_node(map, &canonical_map_expr, "map.sim", 20);
    assert_node(
        &map.children[0],
        &Expr::String("a".to_owned()),
        "map-a-key.sim",
        24,
    );
    assert_node(
        &map.children[1],
        &Expr::List(vec![Expr::Bool(true)]),
        "map-a-value.sim",
        25,
    );
    assert_node(
        &map.children[1].children[0],
        &Expr::Bool(true),
        "map-a-descendant.sim",
        26,
    );
    assert_node(
        &map.children[2],
        &Expr::String("z".to_owned()),
        "map-z-key.sim",
        21,
    );
    assert_node(
        &map.children[3],
        &Expr::List(vec![Expr::Bool(false)]),
        "map-z-value.sim",
        22,
    );
    assert_node(
        &map.children[3].children[0],
        &Expr::Bool(false),
        "map-z-descendant.sim",
        23,
    );

    assert_eq!(encode_located_tree_frame(&decoded, true).unwrap().0, bytes);
}
