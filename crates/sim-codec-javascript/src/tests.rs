use super::*;
use sim_codec::{
    DecodeLimits, Input, Output, decode_located_with_codec, decode_tree_with_codec,
    decode_with_codec, decode_with_codec_and_limits, encode_tree_with_codec, encode_with_codec,
};
use sim_kernel::{EncodeOptions, Expr, ReadPolicy, SourceId, Symbol};

#[test]
fn frozen_authorities_and_manifest_are_exact() {
    assert_eq!(ECMA262_EDITION, "ECMA-262, 17th edition (ECMAScript 2026)");
    assert_eq!(ECMA262_FROZEN_ON, "2026-08-01");
    let manifest = include_bytes!("../grammar/corpus-manifest.txt");
    assert!(!manifest.is_empty());
    assert_eq!(
        CORPUS_MANIFEST_SHA256,
        "88b15b687f3741ba7c0145d1e2f09e05f0b1d8bf95656f257656fe3aefe416da"
    );
}
#[test]
fn corpus_is_byte_stable_and_covers_lexical_goals() {
    let s = include_str!("../tests/corpus.js");
    let t = parse_script(s).unwrap();
    assert_eq!(t.preserve_source().as_bytes(), s.as_bytes());
    assert!(t.tokens.iter().any(|x| x.kind == TokenKind::RegExp));
    assert!(t.tokens.iter().any(|x| x.kind == TokenKind::Template));
    assert!(t.tokens.iter().any(|x| x.goal == LexicalGoal::Div));
    assert!(t.tokens.iter().any(|x| x.goal == LexicalGoal::RegExp));
}
#[test]
fn script_and_module_forms_are_structured() {
    let m =
        parse_module("import x from 'x'; export class C { method(a) { return a + 1; } }").unwrap();
    assert_eq!(m.goal, Goal::Module);
    assert!(
        m.root.children[0]
            .children
            .iter()
            .any(|n| n.kind == NodeKind::Import)
    );
    assert!(
        m.root.children[0]
            .children
            .iter()
            .any(|n| n.kind == NodeKind::Export)
    );
    let s = parse_script("function f(a) { if (a) return a; else return 0; }").unwrap();
    assert_eq!(s.root.kind, NodeKind::Script);
}
#[test]
fn asi_and_early_errors_are_evidence() {
    let t = parse_script("return\nvalue").unwrap();
    assert!(
        t.root.children[0]
            .children
            .iter()
            .any(|n| matches!(n.asi, Some(Asi::LineTerminator(_))))
    );
    assert_eq!(
        parse_script("import x from 'x'").unwrap_err().code,
        DiagnosticCode::EarlyError
    );
    assert_eq!(
        parse_module("with (x) y()").unwrap_err().code,
        DiagnosticCode::EarlyError
    );
    assert_eq!(
        parse_script("throw\nerror").unwrap_err().code,
        DiagnosticCode::EarlyError
    );
}
#[test]
fn malformed_and_limits_fail_deterministically() {
    for s in ["'unterminated", "/unterminated", "({]", "/* nope"] {
        let a = parse_script(s).unwrap_err();
        let b = parse_script(s).unwrap_err();
        assert_eq!(a, b);
        assert!(a.line > 0);
    }
    let l = Limits {
        max_bytes: 4,
        ..Limits::default()
    };
    assert_eq!(
        parse_script_with_limits("12345", l).unwrap_err().code,
        DiagnosticCode::ResourceLimit
    );
    let l = Limits {
        max_tokens: 2,
        ..Limits::default()
    };
    assert_eq!(
        parse_script_with_limits("a + b", l).unwrap_err().code,
        DiagnosticCode::ResourceLimit
    );
    let l = Limits {
        max_nesting: 2,
        ..Limits::default()
    };
    assert_eq!(
        parse_script_with_limits("(((x)))", l).unwrap_err().code,
        DiagnosticCode::ResourceLimit
    );
    let l = Limits {
        max_nodes: 1,
        ..Limits::default()
    };
    assert_eq!(
        parse_script_with_limits("x", l).unwrap_err().code,
        DiagnosticCode::ResourceLimit
    );
}
#[test]
fn neutral_extension_seam_is_constructible() {
    let origin = Origin {
        source: "typed.ts".into(),
        span: Span { start: 0, end: 1 },
        parent: None,
    };
    let node = Node {
        kind: NodeKind::Expression,
        tokens: 0..1,
        children: vec![],
        asi: None,
    };
    assert_eq!(origin.span.end, 1);
    assert_eq!(node.tokens, 0..1);
}

fn codec_cx() -> sim_kernel::Cx {
    let mut cx = sim_test_support::core_cx();
    let id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&JavascriptCodecLib::new(id)).unwrap();
    cx
}

fn javascript_symbol() -> Symbol {
    Symbol::qualified("codec", "javascript")
}
fn output_text(output: Output) -> String {
    match output {
        Output::Text(text) => text,
        Output::Bytes(_) => panic!("JavaScript must be text"),
    }
}

#[test]
fn every_parsed_form_lowers_canonically_without_execution_ir() {
    let source = "function f(a) { return a + 1; } class C {}\n";
    let parsed = parse_script(source).unwrap();
    let builder = JavascriptBuilder;
    let lowered = lower_javascript(&parsed);
    let Expr::Call { args, .. } = &lowered else {
        panic!("lowering must be a form")
    };
    assert_eq!(
        builder.node(&parsed.root.children[0]),
        *args.last().unwrap()
    );
    assert_eq!(output_text(encode_javascript(&lowered).unwrap()), source);
    let debug = format!("{lowered:?}");
    assert!(debug.contains("javascript"));
    for forbidden in ["bytecode", "control-flow", "optimizer", "emit-target"] {
        assert!(!debug.contains(forbidden));
    }
}

#[test]
fn codec_lanes_roundtrip_deterministically_with_origins() {
    let source = "// lead\nconst answer = left + 0x2a;\n";
    let mut cx = codec_cx();
    let symbol = javascript_symbol();
    let lowered = decode_with_codec(
        &mut cx,
        &symbol,
        Input::Text(source.into()),
        ReadPolicy::default(),
    )
    .unwrap();
    let first = output_text(
        encode_with_codec(&mut cx, &symbol, &lowered, EncodeOptions::default()).unwrap(),
    );
    let second = output_text(
        encode_with_codec(&mut cx, &symbol, &lowered, EncodeOptions::default()).unwrap(),
    );
    assert_eq!(first, source);
    assert_eq!(first, second);
    assert_eq!(
        sim_test_support::roundtrip(&mut cx, "javascript", &lowered),
        lowered
    );

    let located = decode_located_with_codec(
        &mut cx,
        &symbol,
        Input::Text(source.into()),
        ReadPolicy::default(),
        "fixture.js",
    )
    .unwrap();
    let origin = located.origin.unwrap();
    assert_eq!(origin.source, SourceId("fixture.js".into()));
    assert_eq!(
        origin.span,
        sim_kernel::Span {
            start: 0,
            end: source.len()
        }
    );

    let tree = decode_tree_with_codec(
        &mut cx,
        &symbol,
        Input::Text(source.into()),
        ReadPolicy::default(),
        "tree.js",
    )
    .unwrap();
    assert!(
        tree.children
            .iter()
            .skip(1)
            .any(|child| child.origin.is_some())
    );
    assert!(
        tree.children
            .iter()
            .flat_map(|child| &child.children)
            .any(|child| child
                .origin
                .as_ref()
                .is_some_and(|o| o.span.end < source.len()))
    );
    let encoded = encode_tree_with_codec(
        &mut cx,
        &symbol,
        &tree,
        EncodeOptions {
            lossless_origin: true,
            ..EncodeOptions::default()
        },
    )
    .unwrap();
    assert_eq!(output_text(encoded), source);
}

#[test]
fn tagged_fallback_malformed_forms_and_decode_bounds_fail_closed() {
    let mut cx = codec_cx();
    let symbol = javascript_symbol();
    let unspellable = Expr::Bytes(vec![0, 1, 255]);
    assert_eq!(
        sim_test_support::roundtrip(&mut cx, "javascript", &unspellable),
        unspellable
    );
    let forged = JavascriptBuilder.form(
        "token",
        vec![
            Expr::Symbol(Symbol::new("identifier")),
            Expr::String("1".into()),
            Expr::Bool(true),
        ],
    );
    assert!(encode_with_codec(&mut cx, &symbol, &forged, EncodeOptions::default()).is_err());
    for malformed in [
        "__sim_expr__({not-json})",
        "__sim_expr__({\"$expr\":\"unknown\"})",
    ] {
        assert!(
            decode_with_codec(
                &mut cx,
                &symbol,
                Input::Text(malformed.into()),
                ReadPolicy::default()
            )
            .is_err()
        );
    }
    let limits = DecodeLimits {
        max_input_bytes: 8,
        max_tokens: 2,
        max_depth: 2,
        ..DecodeLimits::default()
    };
    assert!(
        decode_with_codec_and_limits(
            &mut cx,
            &symbol,
            Input::Text("alpha + beta".into()),
            ReadPolicy::default(),
            limits
        )
        .is_err()
    );
}

#[test]
fn downstream_extension_uses_public_builder_and_preserves_origin_chain() {
    struct TypedNode {
        node: Node,
        type_name: &'static str,
        origin: Origin,
    }
    impl TypedNode {
        fn lower(&self, builder: &JavascriptBuilder) -> Expr {
            builder.form(
                "typed-expression",
                vec![
                    builder.node(&self.node),
                    Expr::Symbol(Symbol::new(self.type_name)),
                ],
            )
        }
    }
    let tree = parse_script("value").unwrap();
    let builder = JavascriptBuilder;
    let parser_origin = Origin {
        source: "fixture.js".into(),
        span: Span { start: 0, end: 5 },
        parent: None,
    };
    let extension = TypedNode {
        node: tree.root.children[0].clone(),
        type_name: "number",
        origin: builder.derived_origin(
            "fixture.ts",
            Span { start: 0, end: 5 },
            Some(parser_origin),
        ),
    };
    let lowered = extension.lower(&builder);
    assert!(format!("{lowered:?}").contains("typed-expression"));
    assert_eq!(
        extension.origin.parent.as_ref().unwrap().source,
        "fixture.js"
    );
}
