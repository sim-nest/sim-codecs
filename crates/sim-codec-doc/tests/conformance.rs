use std::collections::BTreeSet;

use sim_codec_doc::{
    AsciiDocBackend, AttributeEnvelope, BackendStatus, DialectMarkdownBackend, Inline,
    LatexBackend, LinkDialect, MarkdownBackend, MarkdownDialect, MarkupBackend, MarkupBlock,
    MarkupDecodeOptions, MarkupDoc, MarkupEncodeOptions, TypstBackend, backend_catalog,
};
use sim_kernel::Expr;

struct Fixture {
    id: &'static str,
    backend: Box<dyn MarkupBackend>,
    simple: &'static str,
    math_table: &'static str,
    raw_preserve: &'static str,
}

fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            id: "asciidoc",
            backend: Box::new(AsciiDocBackend),
            simple: include_str!("fixtures/simple.adoc"),
            math_table: include_str!("fixtures/math-table.adoc"),
            raw_preserve: include_str!("fixtures/raw-preserve.adoc"),
        },
        Fixture {
            id: "latex",
            backend: Box::new(LatexBackend),
            simple: include_str!("fixtures/simple.tex"),
            math_table: include_str!("fixtures/math-table.tex"),
            raw_preserve: include_str!("fixtures/raw-preserve.tex"),
        },
        Fixture {
            id: "markdown",
            backend: Box::new(MarkdownBackend),
            simple: include_str!("fixtures/simple.md"),
            math_table: include_str!("fixtures/math-table.md"),
            raw_preserve: include_str!("fixtures/raw-preserve.md"),
        },
        Fixture {
            id: "typst",
            backend: Box::new(TypstBackend),
            simple: include_str!("fixtures/simple.typ"),
            math_table: include_str!("fixtures/math-table.typ"),
            raw_preserve: include_str!("fixtures/raw-preserve.typ"),
        },
    ]
}

#[test]
fn all_implemented_backends_roundtrip_simple_fixture() {
    assert_fixture_coverage();
    for fixture in fixtures() {
        assert_semantic_roundtrip(fixture.id, fixture.backend.as_ref(), fixture.simple);
    }
}

#[test]
fn all_implemented_backends_roundtrip_math_table_fixture() {
    assert_fixture_coverage();
    for fixture in fixtures() {
        assert_semantic_roundtrip(fixture.id, fixture.backend.as_ref(), fixture.math_table);
    }
}

#[test]
fn raw_preserve_fixture_names_every_loss() {
    assert_fixture_coverage();
    for fixture in fixtures() {
        let preserve = MarkupDecodeOptions {
            preserve_source: false,
            preserve_raw: true,
        };
        let (_doc, fidelity) = fixture
            .backend
            .decode(fixture.raw_preserve, &preserve)
            .unwrap_or_else(|err| panic!("{} raw preserve fixture failed: {err}", fixture.id));
        assert!(
            fidelity.dropped.is_empty(),
            "{} should preserve raw fragments when requested: {:?}",
            fixture.id,
            fidelity.dropped
        );
        assert!(
            !fidelity.preserved_raw.is_empty(),
            "{} should report preserved raw fragments",
            fixture.id
        );

        let report = MarkupDecodeOptions {
            preserve_source: false,
            preserve_raw: false,
        };
        let (_doc, fidelity) = fixture
            .backend
            .decode(fixture.raw_preserve, &report)
            .unwrap_or_else(|err| panic!("{} raw report fixture failed: {err}", fixture.id));
        assert!(
            !fidelity.dropped.is_empty(),
            "{} should report raw-fragment losses",
            fixture.id
        );
        for loss in &fidelity.dropped {
            assert!(
                !loss.path.trim().is_empty(),
                "{} reported an unnamed raw loss",
                fixture.id
            );
            assert!(
                !loss.reason.trim().is_empty(),
                "{} reported a raw loss without a reason",
                fixture.id
            );
        }
    }
}

fn assert_semantic_roundtrip(id: &str, backend: &dyn MarkupBackend, source: &str) {
    let decode_opts = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, decode_fidelity) = backend
        .decode(source, &decode_opts)
        .unwrap_or_else(|err| panic!("{id} fixture decode failed: {err}"));
    assert!(
        decode_fidelity.dropped.is_empty(),
        "{id} fixture decode dropped semantic content: {:?}",
        decode_fidelity.dropped
    );

    let encode_opts = MarkupEncodeOptions {
        fail_on_loss: true,
        preserve_raw: true,
    };
    let (encoded, encode_fidelity) = backend
        .encode(&doc, &encode_opts)
        .unwrap_or_else(|err| panic!("{id} fixture encode failed: {err}"));
    assert!(
        encode_fidelity.dropped.is_empty(),
        "{id} fixture encode dropped semantic content: {:?}",
        encode_fidelity.dropped
    );

    let (decoded, second_fidelity) = backend
        .decode(&encoded, &decode_opts)
        .unwrap_or_else(|err| panic!("{id} encoded fixture decode failed: {err}"));
    assert!(
        second_fidelity.dropped.is_empty(),
        "{id} encoded fixture decode dropped semantic content: {:?}",
        second_fidelity.dropped
    );
    assert_eq!(semantic_doc(decoded), semantic_doc(doc), "{id} round-trip");
}

fn assert_fixture_coverage() {
    let implemented: BTreeSet<String> = backend_catalog()
        .into_iter()
        .filter(|info| info.status == BackendStatus::Implemented)
        .map(|info| info.id.to_string())
        .collect();
    let fixture_ids: BTreeSet<String> = fixtures()
        .into_iter()
        .map(|fixture| fixture.id.to_owned())
        .collect();
    assert_eq!(fixture_ids, implemented);
}

fn semantic_doc(mut doc: MarkupDoc) -> MarkupDoc {
    doc.source = None;
    doc.blocks = doc.blocks.into_iter().map(block_without_span).collect();
    doc
}

#[test]
fn markdown_dialect_matrix_roundtrips_attrs_and_links() {
    let mut attrs = std::collections::BTreeMap::new();
    attrs.insert("empty".to_owned(), Expr::String(String::new()));
    attrs.insert(
        "quote\"unicode".to_owned(),
        Expr::String("räksmörgås\\\nline".to_owned()),
    );
    attrs.insert(
        "values".to_owned(),
        Expr::List(vec![
            Expr::Nil,
            Expr::Bool(true),
            Expr::String("[]|()".to_owned()),
        ]),
    );
    let doc = MarkupDoc {
        title: None,
        blocks: vec![MarkupBlock::Paragraph {
            content: vec![Inline::Link {
                label: vec![Inline::Text("label | [] () \\ 世界".to_owned())],
                target: "target-unicode-世界".to_owned(),
            }],
            span: None,
        }],
        attrs,
        source: None,
    };
    for attributes in [
        AttributeEnvelope::JsonFrontMatter,
        AttributeEnvelope::DoubleColon,
    ] {
        for links in [LinkDialect::CommonMark, LinkDialect::WikiLink] {
            let backend = DialectMarkdownBackend::new(MarkdownDialect {
                attributes,
                links,
                ..MarkdownDialect::default()
            })
            .unwrap();
            let (encoded, fidelity) = backend
                .encode(&doc, &MarkupEncodeOptions::default())
                .unwrap();
            assert!(fidelity.dropped.is_empty());
            let (decoded, fidelity) = backend
                .decode(
                    &encoded,
                    &MarkupDecodeOptions {
                        preserve_source: false,
                        preserve_raw: true,
                    },
                )
                .unwrap();
            assert!(fidelity.dropped.is_empty());
            assert_eq!(decoded.attrs, doc.attrs);
            assert_eq!(first_link(&decoded), first_link(&doc));
        }
    }
}

fn first_link(doc: &MarkupDoc) -> (String, String) {
    let MarkupBlock::Paragraph { content, .. } = &doc.blocks[0] else {
        panic!("expected paragraph")
    };
    let Inline::Link { label, target } = &content[0] else {
        panic!("expected link")
    };
    let label = label
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => text.as_str(),
            _ => panic!("expected plain link label"),
        })
        .collect();
    (label, target.clone())
}

#[test]
fn markdown_default_is_byte_identical_and_dialect_errors_are_typed() {
    let source = include_str!("fixtures/simple.md");
    let opts = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, old_fidelity) = MarkdownBackend.decode(source, &opts).unwrap();
    let configured = DialectMarkdownBackend::new(MarkdownDialect::default()).unwrap();
    let (same_doc, new_fidelity) = configured.decode(source, &opts).unwrap();
    assert_eq!(doc, same_doc);
    assert_eq!(old_fidelity, new_fidelity);
    assert_eq!(
        MarkdownBackend
            .encode(&doc, &MarkupEncodeOptions::default())
            .unwrap(),
        configured
            .encode(&doc, &MarkupEncodeOptions::default())
            .unwrap()
    );

    let json = DialectMarkdownBackend::new(MarkdownDialect {
        attributes: AttributeEnvelope::JsonFrontMatter,
        ..MarkdownDialect::default()
    })
    .unwrap();
    assert!(
        json.decode(
            "---json\n{\"x\":{\"$expr\":\"nil\"},\"x\":{\"$expr\":\"nil\"}}\n---\nbody",
            &opts
        )
        .is_err()
    );
    let wiki = DialectMarkdownBackend::new(MarkdownDialect {
        links: LinkDialect::WikiLink,
        ..MarkdownDialect::default()
    })
    .unwrap();
    for malformed in ["[[", "[[]]", "[[bad\0link]]", "[[bad\\q]]"] {
        assert!(
            wiki.decode(malformed, &opts).is_err(),
            "accepted {malformed:?}"
        );
    }
}

fn block_without_span(block: MarkupBlock) -> MarkupBlock {
    match block {
        MarkupBlock::Heading {
            level, text, id, ..
        } => MarkupBlock::Heading {
            level,
            text,
            id,
            span: None,
        },
        MarkupBlock::Paragraph { content, .. } => MarkupBlock::Paragraph {
            content,
            span: None,
        },
        MarkupBlock::CodeBlock { lang, code, .. } => MarkupBlock::CodeBlock {
            lang,
            code,
            span: None,
        },
        MarkupBlock::MathBlock { source, .. } => MarkupBlock::MathBlock { source, span: None },
        MarkupBlock::Quote { blocks, .. } => MarkupBlock::Quote {
            blocks: blocks.into_iter().map(block_without_span).collect(),
            span: None,
        },
        MarkupBlock::List { ordered, items, .. } => MarkupBlock::List {
            ordered,
            items: items
                .into_iter()
                .map(|item| item.into_iter().map(block_without_span).collect())
                .collect(),
            span: None,
        },
        MarkupBlock::Table { header, rows, .. } => MarkupBlock::Table {
            header,
            rows,
            span: None,
        },
        MarkupBlock::Figure { src, caption, .. } => MarkupBlock::Figure {
            src,
            caption,
            span: None,
        },
        MarkupBlock::Raw { backend, text, .. } => MarkupBlock::Raw {
            backend,
            text,
            span: None,
        },
    }
}
