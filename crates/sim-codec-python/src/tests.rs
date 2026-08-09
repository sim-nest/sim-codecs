use super::*;

// conformance: Python 3.14 syntax is bounded, located, deterministic, and byte-preserving.

#[test]
fn frozen_identity_and_production_inventory_are_stable() {
    assert_eq!(PYTHON_VERSION, "3.14.6");
    assert!(frozen_productions().len() >= 150);
    let grammar = include_bytes!("../grammar/python-3.14.6.gram");
    let corpus = include_bytes!("../grammar/corpus-3.14.6.txt");
    assert!(!grammar.is_empty() && !corpus.is_empty());
    for production in frozen_productions() {
        assert!(
            grammar
                .windows(production.len())
                .any(|w| w == production.as_bytes()),
            "missing frozen production {production}"
        );
    }
}

#[test]
fn corpus_is_byte_stable_and_covers_modern_tokens() {
    let source = include_str!("../tests/corpus.py");
    let tree = parse_module(source).unwrap();
    assert_eq!(tree.preserve_source().as_bytes(), source.as_bytes());
    assert!(tree.tokens.iter().any(|t| t.kind == TokenKind::FString));
    assert!(
        tree.tokens
            .iter()
            .any(|t| t.kind == TokenKind::TemplateString)
    );
    assert_eq!(tree.source(), source);
}

#[test]
fn trivia_locations_soft_keywords_and_literals_are_lossless() {
    let source = "# lead\nmatch = 0xCA_FE + 1.2e-3j\ncase = rb'bytes'\n";
    let tokens = tokenize(source).unwrap();
    assert_eq!(&source[tokens[0].span.start..tokens[0].span.end], "# lead");
    assert!(tokens.iter().filter(|t| t.kind == TokenKind::Name).count() >= 2);
    assert!(tokens.iter().any(|t| t.kind == TokenKind::Number));
}

#[test]
fn malformed_layout_and_delimiters_are_located_and_deterministic() {
    for source in [
        "if True:\n    x = 1\n  y = 2\n",
        "x = ([)]\n",
        "x = f'{value'\n",
    ] {
        let first = parse_module(source).unwrap_err();
        let second = parse_module(source).unwrap_err();
        assert_eq!(first, second);
        assert!(first.line >= 1);
        assert!(first.span.start <= source.len());
    }
}

#[test]
fn every_resource_limit_fails_closed() {
    let base = Limits {
        max_bytes: 8,
        max_tokens: 100,
        max_nesting: 8,
        max_lines: 8,
    };
    assert_eq!(
        parse_module_with_limits("012345678", base)
            .unwrap_err()
            .code,
        DiagnosticCode::ResourceLimit
    );
    let token_limited = Limits {
        max_bytes: 100,
        max_tokens: 2,
        ..base
    };
    assert_eq!(
        parse_module_with_limits("x = 1", token_limited)
            .unwrap_err()
            .code,
        DiagnosticCode::ResourceLimit
    );
    let nested = Limits {
        max_bytes: 100,
        max_tokens: 100,
        max_nesting: 2,
        max_lines: 8,
    };
    assert_eq!(
        parse_module_with_limits("x = (((1)))", nested)
            .unwrap_err()
            .code,
        DiagnosticCode::ResourceLimit
    );
    let lines = Limits {
        max_bytes: 100,
        max_tokens: 100,
        max_nesting: 8,
        max_lines: 2,
    };
    assert_eq!(
        parse_module_with_limits("x\ny\nz\n", lines)
            .unwrap_err()
            .code,
        DiagnosticCode::ResourceLimit
    );
}
