use super::*;

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
