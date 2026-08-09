use crate::{
    DiagnosticCode, Limits, SyntaxKind, SyntaxNode, TYPESCRIPT_CODEC_ID, TYPESCRIPT_VERSION,
    decode_typescript, decode_typescript_located, decode_typescript_tree, encode_typescript,
    lower_typescript, parse_module, parse_module_with_limits, parse_tsx,
};
use sim_codec::{DecodeBudget, DecodeLimits, Input, Output, ReadCx};
use sim_kernel::{Expr, ReadPolicy, SourceId};

#[test]
fn freezes_typescript_7_identity() {
    assert_eq!(TYPESCRIPT_VERSION, "7.0.2");
}

#[test]
fn represents_declarations_annotations_types_and_modifiers() {
    let source = "declare namespace API { export interface Box<T> { readonly value: T } }\ntype Result<T> = T extends Error ? never : T;\nclass C { public override accessor value: unknown; }";
    let tree = parse_module(source).unwrap();
    for kind in [
        SyntaxKind::Declaration,
        SyntaxKind::Annotation,
        SyntaxKind::TypeArguments,
        SyntaxKind::TypeNode,
        SyntaxKind::Modifier,
    ] {
        assert!(
            tree.nodes.iter().any(
                |node| matches!(node, SyntaxNode::TypeScript { kind: found, .. } if *found == kind)
            ),
            "missing {kind:?}"
        );
    }
    assert!(matches!(tree.nodes[0], SyntaxNode::JavaScript(_)));
    assert_eq!(tree.preserve_source(), source);
}

#[test]
fn tsx_is_mode_bound_and_lossless_with_trivia() {
    let source = "// lead\nconst view = <Panel title=\"x\">{value}</Panel>; // tail\n";
    assert_eq!(
        parse_module(source).unwrap_err().code,
        DiagnosticCode::JsxInTypeScript
    );
    let tree = parse_tsx(source).unwrap();
    assert_eq!(tree.preserve_source(), source);
    assert!(tree.nodes.iter().any(|node| matches!(
        node,
        SyntaxNode::TypeScript {
            kind: SyntaxKind::Jsx,
            ..
        }
    )));
    assert!(tree.tokens.iter().any(|token| token.line == 2));
}

#[test]
fn locations_context_and_bounds_are_stable() {
    let source = "interface Box<T> {\n  value: T\n}";
    let tree = parse_module(source).unwrap();
    let annotation = tree
        .nodes
        .iter()
        .find_map(|node| match node {
            SyntaxNode::TypeScript {
                kind: SyntaxKind::Annotation,
                span,
                context,
            } => Some((span, context)),
            _ => None,
        })
        .unwrap();
    assert_eq!(&source[annotation.0.start..annotation.0.end], ":");
    assert_eq!(annotation.1, &["interface"]);
    let error = parse_module_with_limits(
        source,
        Limits {
            max_bytes: 4,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, DiagnosticCode::ResourceLimit);
}

#[test]
fn admission_is_literally_no_compiler_decision() {
    for (source, construct) in [
        ("enum E { A }", "enum"),
        ("namespace N { export const x = 1 }", "namespace"),
        ("const x = value satisfies T;", "satisfies"),
        ("const x = value as T;", "as"),
        (
            "class C { constructor(public x: number) {} }",
            "parameter property",
        ),
    ] {
        let gap = lower_typescript(&parse_module(source).unwrap()).unwrap_err();
        assert_eq!(gap.construct, construct);
        assert!(
            gap.reason.contains("decision")
                || gap.reason.contains("transform")
                || gap.reason.contains("generation")
                || gap.reason.contains("assignment")
        );
        assert_eq!(
            &source[gap.span.start..gap.span.end],
            if construct == "parameter property" {
                "public"
            } else {
                construct
            }
        );
    }
    assert!(lower_typescript(&parse_module("export * as api from 'pkg';").unwrap()).is_ok());
}

#[test]
fn direct_builder_erasure_retains_annotations_and_origin_chain() {
    let source = "interface Box { value: number }\nfunction id<T>(value: T): T { return value; }";
    let lowered = lower_typescript(&parse_module(source).unwrap()).unwrap();
    assert!(!lowered.annotations.is_empty());
    assert!(
        lowered
            .annotations
            .iter()
            .all(|annotation| annotation.origin.parent.is_some())
    );
    assert_eq!(lowered.source_tree.preserve_source(), source);
    let encoded = match encode_typescript(&lowered.javascript).unwrap() {
        Output::Text(x) => x,
        Output::Bytes(_) => panic!("TypeScript must be text"),
    };
    assert!(!encoded.contains("interface"));
    assert!(!encoded.contains(": T"));
    assert!(encoded.contains("function"));
    assert!(encoded.contains("return value"));
}

#[test]
fn gaps_are_located_and_keep_the_lossless_tree() {
    let source = "// lead\nconst view = <Panel title=\"x\">{value}</Panel>; // tail\n";
    let tree = parse_tsx(source).unwrap();
    let gap = lower_typescript(&tree).unwrap_err();
    assert_eq!(gap.construct, "JSX/TSX");
    assert_eq!(tree.preserve_source(), source);
    assert_eq!(&source[gap.span.start..gap.span.end], "<");
}

#[test]
fn decode_lanes_fallback_and_no_text_pipeline_are_deterministic() {
    let source = "const answer: number = 42;";
    let mut cx = sim_test_support::core_cx();
    let limits = DecodeLimits::default();
    let mut read = ReadCx {
        cx: &mut cx,
        codec: TYPESCRIPT_CODEC_ID,
        read_policy: ReadPolicy::default(),
        limits,
    };
    let mut budget = DecodeBudget::new(limits);
    let plain = decode_typescript(&mut read, source, &mut budget).unwrap();
    let located =
        decode_typescript_located(&mut read, "fixture.ts", Input::Text(source.into())).unwrap();
    let tree = decode_typescript_tree(&mut read, "tree.ts", Input::Text(source.into())).unwrap();
    assert_eq!(located.expr, plain);
    assert_eq!(
        located.origin.unwrap().source,
        SourceId("fixture.ts".into())
    );
    assert_eq!(tree.expr, plain);
    let first = encode_typescript(&plain).unwrap();
    let second = encode_typescript(&plain).unwrap();
    assert_eq!(first, second);
    let fallback = encode_typescript(&Expr::Bool(true)).unwrap();
    let Output::Text(tagged) = fallback else {
        panic!("fallback must be text")
    };
    assert!(tagged.starts_with("__sim_expr__("));
    let mut fallback_budget = DecodeBudget::new(limits);
    assert_eq!(
        decode_typescript(&mut read, &tagged, &mut fallback_budget).unwrap(),
        Expr::Bool(true)
    );
}
