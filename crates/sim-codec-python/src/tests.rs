use super::*;
use sim_codec::{
    DecodeLimits, Input, Output, decode_located_with_codec, decode_tree_with_codec,
    decode_with_codec, decode_with_codec_and_limits, encode_tree_with_codec, encode_with_codec,
};
use sim_kernel::{EncodeOptions, Expr, ReadPolicy, SourceId, Symbol};

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

fn codec_cx() -> sim_kernel::Cx {
    let mut cx = sim_test_support::core_cx();
    let id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&PythonCodecLib::new(id)).unwrap();
    cx
}

fn python_symbol() -> Symbol {
    Symbol::qualified("codec", "python")
}

fn output_text(output: Output) -> String {
    match output {
        Output::Text(text) => text,
        Output::Bytes(_) => panic!("Python source must be textual"),
    }
}

#[test]
fn stable_forms_roundtrip_canonically_and_keep_support_metadata() {
    let source = "answer = left + 0x2a\n";
    let mut cx = codec_cx();
    let symbol = python_symbol();
    let lowered = decode_with_codec(
        &mut cx,
        &symbol,
        Input::Text(source.to_owned()),
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
        sim_test_support::roundtrip(&mut cx, "python", &lowered),
        lowered
    );

    let rendered = format!("{lowered:?}");
    assert!(rendered.contains("python"));
    assert!(rendered.contains("Bool(true)"));
    assert!(rendered.contains("Bool(false)"));
}

#[test]
fn located_and_tree_lanes_retain_source_chain_and_lossless_bytes() {
    let source = "# origin\nvalue = (one + two)\n";
    let mut cx = codec_cx();
    let symbol = python_symbol();
    let located = decode_located_with_codec(
        &mut cx,
        &symbol,
        Input::Text(source.to_owned()),
        ReadPolicy::default(),
        "python-test.py".to_owned(),
    )
    .unwrap();
    let origin = located.origin.unwrap();
    assert_eq!(origin.source, SourceId("python-test.py".to_owned()));
    assert_eq!(origin.span.start, 0);
    assert_eq!(origin.span.end, source.len());

    let tree = decode_tree_with_codec(
        &mut cx,
        &symbol,
        Input::Text(source.to_owned()),
        ReadPolicy::default(),
        "python-tree.py".to_owned(),
    )
    .unwrap();
    assert_eq!(tree.origin.as_ref().unwrap().span.end, source.len());
    assert!(tree.children.iter().any(|child| child.origin.is_some()));
    assert!(
        tree.children
            .iter()
            .flat_map(|child| &child.children)
            .any(|child| child.origin.is_some())
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
fn tagged_fallback_roundtrips_unspellable_expr_and_refuses_malformed_forms() {
    let mut cx = codec_cx();
    let symbol = python_symbol();
    let unspellable = Expr::Bytes(vec![0, 1, 2, 255]);
    assert_eq!(
        sim_test_support::roundtrip(&mut cx, "python", &unspellable),
        unspellable
    );

    let forged = Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("python", "token"))),
        args: vec![
            Expr::Symbol(Symbol::new("name")),
            Expr::String("1".to_owned()),
            Expr::Bool(true),
        ],
    };
    assert!(encode_with_codec(&mut cx, &symbol, &forged, EncodeOptions::default()).is_err());

    for malformed in [
        "__sim_expr__({not-json})",
        "__sim_expr__({\"$expr\":\"unknown\"})",
    ] {
        assert!(
            decode_with_codec(
                &mut cx,
                &symbol,
                Input::Text(malformed.to_owned()),
                ReadPolicy::default(),
            )
            .is_err()
        );
    }
}

#[test]
fn codec_decode_limits_bound_source_tokens_and_fallback_depth() {
    let mut cx = codec_cx();
    let symbol = python_symbol();
    let tiny = DecodeLimits {
        max_input_bytes: 8,
        max_tokens: 2,
        max_depth: 2,
        ..DecodeLimits::default()
    };
    assert!(
        decode_with_codec_and_limits(
            &mut cx,
            &symbol,
            Input::Text("long_name = 1\n".to_owned()),
            ReadPolicy::default(),
            tiny,
        )
        .is_err()
    );
}

#[test]
fn crate_has_no_foreign_python_dependency_or_artifact() {
    use std::{fs, path::Path};

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in ["pyo3", "cpython =", "python3-sys", "python27-sys"] {
        assert!(
            !manifest.contains(forbidden),
            "foreign Python dependency {forbidden}"
        );
    }
    fn scan(path: &Path) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path);
                continue;
            }
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            assert!(
                !matches!(extension, "pyc" | "pyo" | "pickle"),
                "foreign Python artifact {}",
                path.display()
            );
        }
    }
    scan(root);
    let sources = ["src/lib.rs", "src/lower.rs", "src/parser.rs"]
        .into_iter()
        .map(|path| fs::read_to_string(root.join(path)).unwrap())
        .collect::<String>();
    for forbidden in [
        "Py_CompileString",
        "PyEval_",
        "Command::new(\"python\"",
        "Command::new(\"python3\"",
    ] {
        assert!(
            !sources.contains(forbidden),
            "compiler/VM fallback marker {forbidden}"
        );
    }
}
