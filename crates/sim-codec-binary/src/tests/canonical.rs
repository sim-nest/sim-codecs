use super::*;

#[path = "canonical/origin.rs"]
mod origin;

fn strict_decode(
    lane: BinaryFrameLane,
    bytes: &[u8],
) -> sim_kernel::Result<(crate::FrameTables, LocatedExprTree)> {
    decode_canonical_located_tree_frame_with_limits(
        sim_kernel::CodecId(17),
        bytes,
        lane,
        DecodeLimits::default(),
    )
}

fn bare_header() -> Vec<u8> {
    vec![b'S', b'L', b'B', b'8', 1, 0]
}

fn sample_origin(source: &str, start: usize) -> Origin {
    Origin {
        codec: sim_kernel::CodecId(9),
        source: SourceId(source.to_owned()),
        span: Span {
            start,
            end: start + 1,
        },
        trivia: vec![Trivia::Whitespace(" ".to_owned())],
    }
}

fn assert_noncanonical(bytes: &[u8]) {
    let error = strict_decode(BinaryFrameLane::Bare, bytes).unwrap_err();
    match error {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, sim_kernel::CodecId(17));
            assert_eq!(message, "binary frame is not canonical for decoded content");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

fn assert_wrong_lane(expected: BinaryFrameLane, bytes: &[u8], actual: &str, wanted: &str) {
    let error = strict_decode(expected, bytes).unwrap_err();
    match error {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, sim_kernel::CodecId(17));
            assert_eq!(
                message,
                format!("binary frame lane {actual} differs from expected {wanted}")
            );
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn strict_canonical_decode_accepts_every_typed_writer_lane() {
    let expr = Expr::Vector(vec![
        Expr::Symbol(Symbol::qualified("demo", "item")),
        Expr::Bool(true),
    ]);
    let BinaryFrame(bare) = encode_frame(&expr).unwrap();
    let (tables, decoded) = strict_decode(BinaryFrameLane::Bare, &bare).unwrap();
    assert_eq!(decoded.expr, expr);
    assert_eq!(tables.libs, vec!["demo".to_owned()]);
    assert_eq!(tables.symbols, vec![Symbol::qualified("demo", "item")]);
    assert!(tables.number_domains.is_empty());

    let located = LocatedExpr {
        expr: Expr::String("located".to_owned()),
        origin: Some(sample_origin("input.sim", 4)),
    };
    let BinaryFrame(located_bytes) = encode_located_frame(&located, true).unwrap();
    let (_, decoded) = strict_decode(BinaryFrameLane::Located, &located_bytes).unwrap();
    assert_eq!(decoded.located(), located);

    let tree = LocatedExprTree {
        expr: Expr::List(vec![Expr::Bool(false)]),
        origin: None,
        children: vec![LocatedExprTree::without_children(
            Expr::Bool(false),
            Some(sample_origin("tree.sim", 2)),
        )],
    };
    let BinaryFrame(tree_bytes) = encode_located_tree_frame(&tree, true).unwrap();
    let (_, decoded) = strict_decode(BinaryFrameLane::LocatedTree, &tree_bytes).unwrap();
    assert_eq!(decoded, tree);

    let without_origin = LocatedExpr {
        expr: Expr::Bool(true),
        origin: None,
    };
    let BinaryFrame(without_origin_bytes) = encode_located_frame(&without_origin, true).unwrap();
    strict_decode(BinaryFrameLane::Bare, &without_origin_bytes).unwrap();
}

#[test]
fn strict_canonical_decode_refuses_bare_and_empty_tree_lane_substitution() {
    let BinaryFrame(bare) = encode_frame(&Expr::Nil).unwrap();
    let empty_tree = LocatedExprTree::from_expr_recursive(Expr::Nil);
    let BinaryFrame(tree) = encode_located_tree_frame(&empty_tree, true).unwrap();

    assert_ne!(bare, tree);
    assert_eq!(
        strict_decode(BinaryFrameLane::Bare, &bare).unwrap(),
        strict_decode(BinaryFrameLane::LocatedTree, &tree).unwrap()
    );
    assert_wrong_lane(BinaryFrameLane::LocatedTree, &bare, "bare", "located-tree");
    assert_wrong_lane(BinaryFrameLane::Bare, &tree, "located-tree", "bare");
}

#[test]
fn strict_canonical_decode_refuses_root_origin_lane_substitution() {
    let origin = sample_origin("root.sim", 7);
    let located = LocatedExpr {
        expr: Expr::Nil,
        origin: Some(origin.clone()),
    };
    let tree = LocatedExprTree::without_children(Expr::Nil, Some(origin));
    let BinaryFrame(bare_bytes) = encode_frame(&Expr::Nil).unwrap();
    let BinaryFrame(located_bytes) = encode_located_frame(&located, true).unwrap();
    let BinaryFrame(tree_bytes) = encode_located_tree_frame(&tree, true).unwrap();

    assert_eq!(
        strict_decode(BinaryFrameLane::Bare, &bare_bytes)
            .unwrap()
            .1
            .expr,
        strict_decode(BinaryFrameLane::Located, &located_bytes)
            .unwrap()
            .1
            .expr
    );
    assert_wrong_lane(BinaryFrameLane::Bare, &located_bytes, "located", "bare");
    assert_wrong_lane(BinaryFrameLane::Located, &bare_bytes, "bare", "located");
    assert_ne!(located_bytes, tree_bytes);
    assert_eq!(
        strict_decode(BinaryFrameLane::Located, &located_bytes).unwrap(),
        strict_decode(BinaryFrameLane::LocatedTree, &tree_bytes).unwrap()
    );
    assert_wrong_lane(
        BinaryFrameLane::LocatedTree,
        &located_bytes,
        "located",
        "located-tree",
    );
    assert_wrong_lane(
        BinaryFrameLane::Located,
        &tree_bytes,
        "located-tree",
        "located",
    );
}

#[test]
fn strict_canonical_decode_refuses_nonminimal_varuint() {
    let BinaryFrame(canonical) = encode_frame(&Expr::Nil).unwrap();
    let mut alternate = canonical.clone();
    alternate.splice(4..5, [0x81, 0x00]);

    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &alternate).unwrap().1,
        Expr::Nil
    );
    assert_noncanonical(&alternate);
}

#[test]
fn strict_canonical_decode_refuses_unused_and_duplicated_side_tables() {
    let mut unused = bare_header();
    unused.extend_from_slice(&[
        0, // library table
        1, 0, 1, b'x', // one unused bare-symbol record
        0,    // number-domain table
        0,    // nil body
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &unused).unwrap().1,
        Expr::Nil
    );
    assert_noncanonical(&unused);

    let mut duplicate = bare_header();
    duplicate.extend_from_slice(&[
        0, // library table
        2, 0, 1, b'x', 0, 1, b'x', // duplicate symbol records
        0,    // number-domain table
        4, 0, // symbol body referencing the first record
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &duplicate).unwrap().1,
        Expr::Symbol(Symbol::new("x"))
    );
    assert_noncanonical(&duplicate);

    let mut unused_lib = bare_header();
    unused_lib.extend_from_slice(&[1, 1, b'x', 0, 0, 0]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &unused_lib)
            .unwrap()
            .1,
        Expr::Nil
    );
    assert_noncanonical(&unused_lib);

    let mut duplicate_lib = bare_header();
    duplicate_lib.extend_from_slice(&[
        2, 1, b'x', 1, b'x', // duplicate library records
        1, 1, 1, b'y', // one x/y symbol using the first library record
        0,    // number-domain table
        4, 0, // symbol body
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &duplicate_lib)
            .unwrap()
            .1,
        Expr::Symbol(Symbol::qualified("x", "y"))
    );
    assert_noncanonical(&duplicate_lib);

    let mut unused_domain = bare_header();
    unused_domain.extend_from_slice(&[
        1, 7, b'n', b'u', b'm', b'b', b'e', b'r', b's', // libraries
        0,    // symbols
        1, 1, 3, b'f', b'6', b'4', // one unused number domain
        0,    // nil body
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &unused_domain)
            .unwrap()
            .1,
        Expr::Nil
    );
    assert_noncanonical(&unused_domain);

    let mut duplicate_domain = bare_header();
    duplicate_domain.extend_from_slice(&[
        1, 7, b'n', b'u', b'm', b'b', b'e', b'r', b's', // libraries
        0,    // symbols
        2, 1, 3, b'f', b'6', b'4', 1, 3, b'f', b'6', b'4', // domains
        3, 0, 1, b'1', // number body
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &duplicate_domain)
            .unwrap()
            .1,
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1".to_owned(),
        })
    );
    assert_noncanonical(&duplicate_domain);
}

#[test]
fn strict_canonical_decode_refuses_reordered_side_tables_with_remapped_body() {
    let mut reordered_symbols = bare_header();
    reordered_symbols.extend_from_slice(&[
        0, // library table
        2, 0, 1, b'b', 0, 1, b'a', // reverse canonical symbol order
        0,    // number-domain table
        8, 2, // vector with two entries
        4, 1, // a (remapped)
        4, 0, // b (remapped)
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &reordered_symbols)
            .unwrap()
            .1,
        Expr::Vector(vec![
            Expr::Symbol(Symbol::new("a")),
            Expr::Symbol(Symbol::new("b")),
        ])
    );
    assert_noncanonical(&reordered_symbols);

    let mut reordered_libs = bare_header();
    reordered_libs.extend_from_slice(&[
        2, 1, b'z', 1, b'a', // reverse canonical library order
        2, 2, 1, b'x', 1, 1, b'x', // symbols a/x then z/x via remapped libs
        0,    // number-domain table
        8, 2, 4, 0, 4, 1, // vector body: a/x then z/x
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &reordered_libs)
            .unwrap()
            .1,
        Expr::Vector(vec![
            Expr::Symbol(Symbol::qualified("a", "x")),
            Expr::Symbol(Symbol::qualified("z", "x")),
        ])
    );
    assert_noncanonical(&reordered_libs);

    let mut reordered_domains = bare_header();
    reordered_domains.extend_from_slice(&[
        1, 7, b'n', b'u', b'm', b'b', b'e', b'r', b's', // libraries
        0,    // symbols
        2, 1, 1, b'z', 1, 1, b'a', // reverse canonical domain order
        8, 2, // vector body
        3, 1, 1, b'1', // numbers/a via remapped index
        3, 0, 1, b'2', // numbers/z via remapped index
    ]);
    assert_eq!(
        decode_frame(sim_kernel::CodecId(17), &reordered_domains)
            .unwrap()
            .1,
        Expr::Vector(vec![
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "a"),
                canonical: "1".to_owned(),
            }),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "z"),
                canonical: "2".to_owned(),
            }),
        ])
    );
    assert_noncanonical(&reordered_domains);
}

#[test]
fn strict_canonical_decode_refuses_alternate_equivalent_collection_order() {
    let mut alternate_set = bare_header();
    alternate_set.extend_from_slice(&[
        0, // library table
        0, // symbol table
        0, // number-domain table
        10, 2, // set with two entries
        2, // true, before false
        1, // false
    ]);
    let decoded = decode_frame(sim_kernel::CodecId(17), &alternate_set)
        .unwrap()
        .1;
    assert!(decoded.canonical_eq(&Expr::Set(vec![Expr::Bool(false), Expr::Bool(true)])));
    assert_noncanonical(&alternate_set);

    let mut alternate_map = bare_header();
    alternate_map.extend_from_slice(&[
        0, // library table
        0, // symbol table
        0, // number-domain table
        9, 2, // map with two entries
        2, 0, // true => nil, before false => nil
        1, 0,
    ]);
    let decoded = decode_frame(sim_kernel::CodecId(17), &alternate_map)
        .unwrap()
        .1;
    assert!(decoded.canonical_eq(&Expr::Map(vec![
        (Expr::Bool(false), Expr::Nil),
        (Expr::Bool(true), Expr::Nil),
    ])));
    assert_noncanonical(&alternate_map);
}

#[test]
fn strict_canonical_decode_refuses_combined_origin_flags() {
    let mut bytes = vec![b'S', b'L', b'B', b'8', 1, 3, 0, 0, 0, 0];
    bytes.extend_from_slice(&[
        0, // tree origin absent
        0, // root codec
        0, // root source
        0, // root span start
        0, // root span end
        0, // root trivia count
    ]);
    let error = strict_decode(BinaryFrameLane::LocatedTree, &bytes).unwrap_err();
    match error {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, sim_kernel::CodecId(17));
            assert_eq!(
                message,
                "binary frame flags 3 have no canonical writer lane"
            );
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn strict_canonical_decode_preserves_trailing_data_refusal() {
    let BinaryFrame(mut bytes) = encode_frame(&Expr::Bool(true)).unwrap();
    bytes.push(0);
    let error = strict_decode(BinaryFrameLane::Bare, &bytes).unwrap_err();
    match error {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, sim_kernel::CodecId(17));
            assert_eq!(message, "trailing bytes after binary payload");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn strict_canonical_decode_applies_bounds_before_reencoding() {
    let BinaryFrame(bytes) = encode_frame(&Expr::String("bounded".to_owned())).unwrap();
    let error = decode_canonical_located_tree_frame_with_limits(
        sim_kernel::CodecId(17),
        &bytes,
        BinaryFrameLane::Bare,
        DecodeLimits {
            max_string_bytes: 3,
            ..DecodeLimits::default()
        },
    )
    .unwrap_err();
    match error {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, sim_kernel::CodecId(17));
            assert!(message.contains("string exceeds decode limit"));
        }
        other => panic!("unexpected error {other:?}"),
    }
}
