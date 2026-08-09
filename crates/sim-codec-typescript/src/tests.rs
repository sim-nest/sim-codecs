use crate::{
    DiagnosticCode, Limits, SyntaxKind, SyntaxNode, TYPESCRIPT_VERSION, parse_module,
    parse_module_with_limits, parse_tsx,
};

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
