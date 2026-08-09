use sim_codec_javascript::{Goal, LexicalGoal, NodeKind, TokenKind, parse_module, parse_script};

// conformance: frozen ECMAScript source is bounded, located, and byte-preserving.
#[test]
fn lossless_script_and_module_frontend() {
    let script = include_str!("corpus.js");
    let tree = parse_script(script).expect("curated Script corpus");
    assert_eq!(tree.preserve_source(), script);
    assert_eq!(tree.goal, Goal::Script);
    assert!(
        tree.tokens
            .iter()
            .any(|token| token.kind == TokenKind::RegExp)
    );
    assert!(
        tree.tokens
            .iter()
            .any(|token| token.goal == LexicalGoal::Div)
    );

    let module = parse_module("import x from 'x'; export default class C {}").unwrap();
    assert_eq!(module.root.kind, NodeKind::Module);
}
