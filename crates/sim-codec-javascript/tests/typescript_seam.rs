use sim_codec_javascript::{
    JavascriptBuilder, Node, Origin, Span, SyntaxTree, Token, lower_javascript, parse_script,
};
use sim_kernel::Expr;

// A downstream TypeScript wrapper inside the codec's test crate. Its imports
// are the complete frozen seam, with no JavaScript dependency on TypeScript.
struct DownstreamTypeScriptFixture {
    tokens: Vec<Token>,
    tree: SyntaxTree,
    node: Node,
    origin: Origin,
}

impl DownstreamTypeScriptFixture {
    fn erase(self) -> Expr {
        let builder = JavascriptBuilder;
        assert!(!self.tokens.is_empty());
        assert_eq!(self.origin.source, "fixture.ts");
        builder.form(
            "typescript-erased",
            vec![builder.node(&self.node), lower_javascript(&self.tree)],
        )
    }
}

#[test]
fn typescript_consumes_the_neutral_seam_one_way() {
    let tree = parse_script("const answer = 42;").unwrap();
    let fixture = DownstreamTypeScriptFixture {
        tokens: tree.tokens.clone(),
        node: tree.root.clone(),
        origin: JavascriptBuilder.derived_origin("fixture.ts", Span { start: 0, end: 18 }, None),
        tree,
    };
    assert!(matches!(fixture.erase(), Expr::Call { .. }));
}

#[test]
fn codec_manifest_has_no_reverse_typescript_dependency() {
    assert!(
        !include_str!("../Cargo.toml")
            .to_ascii_lowercase()
            .contains("typescript")
    );
}
