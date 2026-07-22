use std::sync::Arc;

use sim_codec::{DecodePosition, encode_with_codec, grammar_check};
use sim_codec_json::{JsonCodecLib, JsonGrammarRenderer};
use sim_codec_lisp::{LispCodecLib, LispGrammarRenderer};
use sim_kernel::{Cx, EncodeOptions, Expr, NumberLiteral, Symbol};
use sim_shape::{
    ExprKind, ExprKindShape, GrammarDialect, GrammarGraph, GrammarPosition, GrammarRenderer,
    GrammarTarget, ListShape, OneOfShape, Production, Shape, ShapeDefRef, ShapeDefs, TerminalAtom,
    shape_grammar,
};

#[test]
fn generated_corpus_round_trips_through_checker() {
    for case in renderer_cases() {
        let mut cx = cx();
        for shape in [flat_shape(), recursive_shape()] {
            let grammar = render_shape(shape.as_ref(), &case);
            assert_eq!(grammar.target.codec, case.codec);
            assert_eq!(grammar.target.dialect, case.dialect);
            assert!(!grammar.text.is_empty());
            assert_eq!(grammar.graph, graph_for(shape.as_ref()));

            let generated = enumerate_graph(&grammar.graph, 4);
            assert!(
                !generated.is_empty(),
                "generated corpus for {} must not be empty",
                case.codec
            );
            for expr in generated {
                let text = encode_text(&mut cx, &case.codec, &expr);
                let check = grammar_check(
                    &mut cx,
                    shape.as_ref(),
                    &case.codec,
                    &text,
                    DecodePosition::Data,
                )
                .unwrap_or_else(|err| panic!("check {} text {text:?}: {err}", case.codec));
                assert!(
                    check.accepted,
                    "shape grammar example {expr:?} failed for {} with {:?}",
                    case.codec, check.diagnostics
                );
                assert!(check.decoded.is_some());
            }
        }
    }
}

#[test]
fn representative_members_are_generated_and_checked() {
    let representatives = [
        (
            flat_shape(),
            Expr::List(vec![text("text"), Expr::Bool(true)]),
        ),
        (recursive_shape(), generated_nested_node(3)),
    ];
    for case in renderer_cases() {
        let mut cx = cx();
        for (shape, expr) in &representatives {
            let grammar = render_shape(shape.as_ref(), &case);
            let generated = enumerate_graph(&grammar.graph, 4);
            assert!(
                generated.contains(expr),
                "representative {expr:?} missing from generated corpus for {}",
                case.codec
            );
            let text = encode_text(&mut cx, &case.codec, expr);
            let check = grammar_check(
                &mut cx,
                shape.as_ref(),
                &case.codec,
                &text,
                DecodePosition::Data,
            )
            .unwrap();
            assert!(check.accepted, "{:?}", check.diagnostics);
        }
    }
}

#[test]
fn decode_failures_and_wrong_shape_text_are_distinct_reports() {
    for case in renderer_cases() {
        let mut cx = cx();
        let shape = flat_shape();
        let decode_failure = grammar_check(
            &mut cx,
            shape.as_ref(),
            &case.codec,
            case.malformed,
            DecodePosition::Data,
        )
        .unwrap();
        assert!(!decode_failure.accepted);
        assert!(decode_failure.decoded.is_none());
        assert!(!decode_failure.diagnostics.is_empty());

        let wrong_shape = Expr::List(vec![text("alpha"), text("not-bool")]);
        let text = encode_text(&mut cx, &case.codec, &wrong_shape);
        let shape_failure = grammar_check(
            &mut cx,
            shape.as_ref(),
            &case.codec,
            &text,
            DecodePosition::Data,
        )
        .unwrap();
        assert!(!shape_failure.accepted);
        assert!(shape_failure.decoded.is_some());
        assert!(!shape_failure.diagnostics.is_empty());
    }
}

#[test]
fn recursive_corpus_agrees_at_and_past_depth_bound() {
    for case in renderer_cases() {
        let mut cx = cx();
        let shape = recursive_shape();

        let accepted = nested_node(4);
        let text = encode_text(&mut cx, &case.codec, &accepted);
        let check = grammar_check(
            &mut cx,
            shape.as_ref(),
            &case.codec,
            &text,
            DecodePosition::Data,
        )
        .unwrap();
        assert!(check.accepted, "{:?}", check.diagnostics);
    }

    let mut lisp_cx = cx();
    let shape = recursive_shape();
    let over_bound = nested_node(70);
    let text = encode_text(&mut lisp_cx, &q("codec", "lisp"), &over_bound);
    let check = grammar_check(
        &mut lisp_cx,
        shape.as_ref(),
        &q("codec", "lisp"),
        &text,
        DecodePosition::Data,
    )
    .unwrap();
    assert!(!check.accepted);
    assert!(check.decoded.is_some());
    assert!(
        check
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("recursion budget")),
        "{:?}",
        check.diagnostics
    );

    let mut json_cx = cx();
    let text = encode_text(&mut json_cx, &q("codec", "json"), &over_bound);
    let check = grammar_check(
        &mut json_cx,
        shape.as_ref(),
        &q("codec", "json"),
        &text,
        DecodePosition::Data,
    )
    .unwrap();
    assert!(!check.accepted);
    assert!(!check.diagnostics.is_empty());
}

struct RendererCase {
    codec: Symbol,
    dialect: GrammarDialect,
    malformed: &'static str,
    renderer: Box<dyn GrammarRenderer>,
}

fn renderer_cases() -> Vec<RendererCase> {
    vec![
        RendererCase {
            codec: q("codec", "json"),
            dialect: GrammarDialect::JsonSchema,
            malformed: "{",
            renderer: Box::new(JsonGrammarRenderer::json_schema()),
        },
        RendererCase {
            codec: q("codec", "lisp"),
            dialect: GrammarDialect::SExpr,
            malformed: "(",
            renderer: Box::new(LispGrammarRenderer::sexpr()),
        },
    ]
}

fn cx() -> Cx {
    let mut cx = sim_test_support::core_cx();
    sim_test_support::register_f64_number_domain(&mut cx);
    let json = JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&json).unwrap();
    let lisp = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    cx
}

fn render_shape(shape: &dyn Shape, case: &RendererCase) -> sim_shape::ShapeGrammar {
    shape_grammar(
        shape,
        GrammarTarget {
            codec: case.codec.clone(),
            dialect: case.dialect,
            position: GrammarPosition::Data,
        },
        case.renderer.as_ref(),
    )
    .unwrap()
}

fn graph_for(shape: &dyn Shape) -> GrammarGraph {
    sim_shape::shape_grammar_graph(shape).unwrap()
}

fn flat_shape() -> Arc<dyn Shape> {
    Arc::new(ListShape::new(vec![string_shape(), bool_shape()]))
}

fn recursive_shape() -> Arc<dyn Shape> {
    let node = q("shape", "Node");
    let node_ref: Arc<dyn Shape> = Arc::new(ShapeDefRef::new(node.clone()));
    let recursive_list: Arc<dyn Shape> =
        Arc::new(ListShape::new(vec![string_shape(), node_ref.clone()]));
    let node_shape: Arc<dyn Shape> =
        Arc::new(OneOfShape::new(vec![string_shape(), recursive_list]));
    Arc::new(ShapeDefs::new(
        Arc::new(ShapeDefRef::new(node.clone())),
        vec![(node, node_shape)],
    ))
}

fn string_shape() -> Arc<dyn Shape> {
    Arc::new(ExprKindShape::new(ExprKind::String))
}

fn bool_shape() -> Arc<dyn Shape> {
    Arc::new(ExprKindShape::new(ExprKind::Bool))
}

fn enumerate_graph(graph: &GrammarGraph, depth: usize) -> Vec<Expr> {
    enumerate_production(&graph.root, graph, depth)
}

fn enumerate_production(production: &Production, graph: &GrammarGraph, depth: usize) -> Vec<Expr> {
    match production {
        Production::Terminal(atom) => terminal_examples(atom),
        Production::Seq(items) => sequence_examples(items, graph, depth),
        Production::Alt(choices) => choices
            .iter()
            .flat_map(|choice| enumerate_production(choice, graph, depth))
            .collect(),
        Production::Repeat { inner, at_least } => {
            if *at_least == 0 {
                vec![Expr::List(Vec::new())]
            } else {
                enumerate_production(inner, graph, depth)
            }
        }
        Production::Call { head, args } => {
            let mut items = enumerate_production(head, graph, depth);
            items.extend(sequence_items(args, graph, depth));
            vec![Expr::List(items)]
        }
        Production::Ref(name) if depth > 0 => graph
            .defs
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, production)| enumerate_production(production, graph, depth - 1))
            .unwrap_or_default(),
        Production::Ref(_) => Vec::new(),
    }
}

fn sequence_examples(items: &[Production], graph: &GrammarGraph, depth: usize) -> Vec<Expr> {
    let variants = items
        .iter()
        .map(|item| enumerate_production(item, graph, depth))
        .collect::<Vec<_>>();
    cartesian_items(&variants)
        .into_iter()
        .map(Expr::List)
        .collect()
}

fn sequence_items(items: &[Production], graph: &GrammarGraph, depth: usize) -> Vec<Expr> {
    cartesian_items(
        &items
            .iter()
            .map(|item| enumerate_production(item, graph, depth))
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .next()
    .unwrap_or_default()
}

fn cartesian_items(variants: &[Vec<Expr>]) -> Vec<Vec<Expr>> {
    let mut out = vec![Vec::new()];
    for choices in variants {
        let mut next = Vec::new();
        for prefix in &out {
            for choice in choices {
                let mut item = prefix.clone();
                item.push(choice.clone());
                next.push(item);
            }
        }
        out = next;
    }
    out
}

fn terminal_examples(atom: &TerminalAtom) -> Vec<Expr> {
    vec![match atom {
        TerminalAtom::Any => text("any"),
        TerminalAtom::Symbol => Expr::Symbol(q("demo", "symbol")),
        TerminalAtom::String => text("text"),
        TerminalAtom::Number => Expr::Number(NumberLiteral {
            domain: q("numbers", "f64"),
            canonical: "1.0".to_owned(),
        }),
        TerminalAtom::Bool => Expr::Bool(true),
        TerminalAtom::Nil => Expr::Nil,
        TerminalAtom::List => Expr::List(Vec::new()),
        TerminalAtom::Map => Expr::Map(Vec::new()),
        TerminalAtom::Exact(expr) => expr.clone(),
    }]
}

fn encode_text(cx: &mut Cx, codec: &Symbol, expr: &Expr) -> String {
    encode_with_codec(cx, codec, expr, EncodeOptions::default())
        .unwrap()
        .into_text()
        .unwrap()
}

fn nested_node(depth: usize) -> Expr {
    if depth == 0 {
        text("leaf")
    } else {
        Expr::List(vec![text("node"), nested_node(depth - 1)])
    }
}

fn generated_nested_node(depth: usize) -> Expr {
    if depth == 0 {
        text("text")
    } else {
        Expr::List(vec![text("text"), generated_nested_node(depth - 1)])
    }
}

fn text(value: &str) -> Expr {
    Expr::String(value.to_owned())
}

fn q(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}
