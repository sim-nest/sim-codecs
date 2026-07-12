use std::sync::Arc;

use sim_codec::{
    DecodeLimits, DecodePosition, DecodedForm, Input, decode_default_with_codec,
    decode_term_with_codec, decode_with_codec, decode_with_codec_and_limits, encode_with_codec,
};
use sim_kernel::{
    Datum, DefaultFactory, EagerPolicy, Expr, NumberLiteral, QuoteMode, Ref, SourceId, Symbol,
    Term, Trivia,
};
use sim_shape::{ExprKind, ExprKindShape, Shape};

use crate::*;

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    sim_test_support::register_f64_number_domain(&mut cx);
    let lisp = sim_codec_lisp::LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    let algol = AlgolCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&algol).unwrap();
    cx
}

#[test]
fn algol_text_parsing_is_codec_local_and_domain_aware() {
    let mut cx = cx();
    let expr = parse_algol_expr_with_table(&mut cx, default_pratt_table(), "1 + 2 * 3").unwrap();
    assert!(matches!(expr, Expr::Infix { .. }));
}

#[test]
fn algol_codec_parses_and_encodes_arithmetic() {
    let mut cx = cx();
    let expr = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text("1 + 2 * 3".to_owned()),
        Default::default(),
    )
    .unwrap();
    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        &expr,
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(encoded, "1 + 2 * 3");
}

#[test]
fn algol_codec_supports_calls_and_prefix_postfix() {
    let mut cx = cx();
    let expr = parse_algol_expr_with_table(&mut cx, default_pratt_table(), "-f(1, 2)!").unwrap();
    assert!(matches!(expr, Expr::Prefix { .. }));
}

#[test]
fn algol_codec_escapes_unsupported_exprs() {
    let mut cx = cx();
    let expr = Expr::Quote {
        mode: QuoteMode::Quote,
        expr: Box::new(Expr::Symbol(Symbol::new("x"))),
    };
    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        &expr,
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(encoded, "expr.lisp(\"(quote x)\")");
    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(encoded),
        Default::default(),
    )
    .unwrap();
    assert!(decoded.canonical_eq(&expr));
}

#[test]
fn algol_codec_escapes_numbers_when_text_would_change_domain() {
    let mut cx = cx();
    let expr = Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: "42".to_owned(),
    });

    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        &expr,
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert!(encoded.starts_with("expr.lisp("), "{encoded}");

    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(encoded),
        Default::default(),
    )
    .unwrap();
    assert_eq!(decoded, expr);
}

#[test]
fn algol_codec_escapes_symbols_when_text_would_change_expr_kind() {
    let mut cx = cx();
    let expr = Expr::Symbol(Symbol::new("nil"));

    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        &expr,
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert!(encoded.contains("expr:symbol"), "{encoded}");

    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(encoded),
        Default::default(),
    )
    .unwrap();
    assert_eq!(decoded, expr);
}

#[test]
fn algol_codec_roundtrips_common_string_escapes() {
    let mut cx = cx();
    let expr = Expr::String("slash\\ quote\" tab\t cr\r lf\n bell\u{7}".to_owned());
    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        &expr,
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(
        encoded,
        "\"slash\\\\ quote\\\" tab\\t cr\\r lf\\n bell\\u{7}\""
    );
    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(encoded),
        Default::default(),
    )
    .unwrap();
    assert_eq!(decoded, expr);
}

#[test]
fn algol_default_decode_uses_term_in_eval_and_datum_in_data() {
    let mut cx = cx();

    let eval = decode_default_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text("1 + 2".to_owned()),
        Default::default(),
        DecodePosition::Eval,
    )
    .unwrap();
    assert!(matches!(eval, DecodedForm::Term(Term::Call { .. })));

    let data = decode_default_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text("\"label\"".to_owned()),
        Default::default(),
        DecodePosition::Data,
    )
    .unwrap();
    assert_eq!(data, DecodedForm::Datum(Datum::String("label".to_owned())));
}

#[test]
fn algol_term_decode_lowers_infix_surface() {
    let mut cx = cx();
    let term = decode_term_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text("1 + 2".to_owned()),
        Default::default(),
    )
    .unwrap();

    let Term::Call { target, args } = term else {
        panic!("expected call term");
    };
    assert_eq!(*target, Term::Ref(Ref::Symbol(Symbol::new("+"))));
    assert_eq!(args.len(), 2);
}

#[test]
fn algol_codec_is_registered() {
    let cx = cx();
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "algol"))
            .is_some()
    );
}

#[test]
fn arithmetic_pratt_table_is_exported_as_value() {
    let cx = cx();
    let value = cx
        .registry()
        .value_by_symbol(&Symbol::qualified("pratt", "arithmetic"))
        .unwrap();
    let table = value
        .object()
        .downcast_ref::<sim_kernel::PrattTableObject>()
        .unwrap();

    assert!(
        table
            .table
            .operators()
            .iter()
            .any(|operator| operator.symbol == Symbol::new("+"))
    );
}

#[test]
fn algol_pratt_shape_adapter_parses_before_matching() {
    let mut cx = cx();
    let shape = algol_pratt_shape(
        default_pratt_table(),
        Arc::new(ExprKindShape::new(ExprKind::Infix)),
    );

    let matched = shape
        .check_expr(&mut cx, &Expr::String("1 + 2 * 3".to_owned()))
        .unwrap();

    assert!(matched.accepted);
}

#[test]
fn located_decode_captures_top_level_origin_and_comments() {
    let located = decode_algol_located(
        sim_kernel::CodecId(2),
        "calc.alg",
        " // lead\n1 + 2 /* tail */  ",
    )
    .unwrap();

    assert!(matches!(located.expr, Expr::Infix { .. }));
    let origin = located.origin.unwrap();
    assert_eq!(origin.source, SourceId("calc.alg".to_owned()));
    assert_eq!(origin.span.start, 9);
    assert_eq!(origin.span.end, 14);
    assert!(!origin.trivia.is_empty());
}

#[test]
fn tree_decode_captures_child_spans() {
    let parser = PrattParser::new(default_pratt_table());
    let tree = parser
        .parse_text_tree(sim_kernel::CodecId(2), "calc.alg", "1 + 2 * 3")
        .unwrap();

    assert_eq!(tree.origin.as_ref().unwrap().span.start, 0);
    assert_eq!(tree.origin.as_ref().unwrap().span.end, 9);
    assert_eq!(tree.children.len(), 2);
    assert_eq!(tree.children[0].origin.as_ref().unwrap().span.start, 0);
    assert_eq!(tree.children[0].origin.as_ref().unwrap().span.end, 1);
    assert_eq!(tree.children[1].origin.as_ref().unwrap().span.start, 4);
    assert_eq!(tree.children[1].origin.as_ref().unwrap().span.end, 9);
    assert_eq!(
        tree.children[1].children[0]
            .origin
            .as_ref()
            .unwrap()
            .span
            .start,
        4
    );
    assert_eq!(
        tree.children[1].children[0]
            .origin
            .as_ref()
            .unwrap()
            .span
            .end,
        5
    );
    assert_eq!(
        tree.children[1].children[1]
            .origin
            .as_ref()
            .unwrap()
            .span
            .start,
        8
    );
    assert_eq!(
        tree.children[1].children[1]
            .origin
            .as_ref()
            .unwrap()
            .span
            .end,
        9
    );
}

#[test]
fn tree_decode_attaches_leading_trivia_to_child_nodes() {
    let parser = PrattParser::new(default_pratt_table());
    let tree = parser
        .parse_text_tree(sim_kernel::CodecId(2), "calc.alg", "1 + /* note */ 2")
        .unwrap();

    let trivia = &tree.children[1].origin.as_ref().unwrap().trivia;
    assert!(!trivia.is_empty());
    assert!(
        trivia
            .iter()
            .any(|item| matches!(item, Trivia::BlockComment(_)))
    );
}

#[test]
fn tree_decode_duplicates_right_side_trivia_into_expression_context() {
    let parser = PrattParser::new(default_pratt_table());
    let tree = parser
        .parse_text_tree(sim_kernel::CodecId(2), "calc.alg", "1 + /* note */ 2")
        .unwrap();

    let parent_trivia = &tree.origin.as_ref().unwrap().trivia;
    assert!(
        parent_trivia
            .iter()
            .any(|item| matches!(item, Trivia::BlockComment(text) if text.contains("note")))
    );
}

#[test]
fn tree_decode_duplicates_close_trivia_into_call_context() {
    let parser = PrattParser::new(default_pratt_table());
    let tree = parser
        .parse_text_tree(sim_kernel::CodecId(2), "calc.alg", "f(1 /* tail */)")
        .unwrap();

    let parent_trivia = &tree.origin.as_ref().unwrap().trivia;
    assert!(
        parent_trivia
            .iter()
            .any(|item| matches!(item, Trivia::BlockComment(text) if text.contains("tail")))
    );
    let arg_trivia = &tree.children[1].origin.as_ref().unwrap().trivia;
    assert!(
        arg_trivia
            .iter()
            .any(|item| matches!(item, Trivia::BlockComment(text) if text.contains("tail")))
    );
}

#[test]
fn algol_decode_rejects_excessive_tokens() {
    let mut cx = cx();
    let limits = DecodeLimits {
        max_tokens: 5,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text("1 + 2 + 3 + 4".to_owned()),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("tokens"))
    );
}

#[test]
fn algol_decode_rejects_excessive_depth() {
    let mut cx = cx();
    let nested = "(".repeat(12) + "1" + &")".repeat(12);
    let limits = DecodeLimits {
        max_depth: 4,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(nested),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("recursion depth"))
    );
}

#[test]
fn algol_decode_rejects_unbalanced_open_parens_without_stack_overflow() {
    // An open-paren chain never reaches a leaf arm, so before the top-level
    // depth charge it recursed to a native stack overflow (an uncatchable abort)
    // rather than returning an error. With the budget charged at the top of
    // `parse_expr_tree` the depth limit is now consulted on this path and the
    // 100k-deep bomb fails closed well before exhausting the stack.
    let mut cx = cx();
    let bomb = "(".repeat(100_000);
    let limits = DecodeLimits {
        max_depth: 128,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(bomb),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("recursion depth"))
    );
}

#[test]
fn algol_decode_rejects_unbalanced_prefix_ops_without_stack_overflow() {
    let mut cx = cx();
    let bomb = "- ".repeat(100_000) + "1";
    // Raise the trivia ceiling so the inter-operator spaces do not trip the
    // trivia budget first; the point of this test is the recursion-depth guard.
    let limits = DecodeLimits {
        max_depth: 128,
        max_trivia_items: 1_000_000,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(bomb),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("recursion depth"))
    );
}

#[test]
fn algol_decode_rejects_excessive_trivia_items() {
    let mut cx = cx();
    let source = "/*a*/".repeat(8) + "1";
    let limits = DecodeLimits {
        max_trivia_items: 4,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "algol"),
        Input::Text(source),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("trivia items"))
    );
}

#[test]
fn algol_rejects_unterminated_block_comment_before_expr() {
    let err = tokenize_algol_spanned("/* open\n1").unwrap_err();
    assert!(format!("{err}").contains("unterminated"));
}

#[test]
fn algol_rejects_unterminated_block_comment_between_tokens() {
    let err = tokenize_algol_spanned("1 + /* open\n2").unwrap_err();
    assert!(format!("{err}").contains("unterminated"));
}

#[test]
fn algol_rejects_unterminated_block_comment_after_expr() {
    let err = tokenize_algol_spanned("1 /* unterminated").unwrap_err();
    assert!(format!("{err}").contains("unterminated"));
}

#[test]
fn algol_accepts_closed_block_comment() {
    let tokens = tokenize_algol_spanned("1 /* closed */").unwrap();
    assert_eq!(tokens.len(), 1);
    assert!(matches!(tokens[0].token, sim_kernel::PrattToken::Number(_)));
}
