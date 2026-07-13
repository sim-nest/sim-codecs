use std::collections::BTreeSet;

use sim_codec_doc::{
    AsciiDocBackend, BackendStatus, LatexBackend, MarkdownBackend, MarkupBackend, MarkupBlock,
    MarkupDecodeOptions, MarkupDoc, MarkupEncodeOptions, TypstBackend, backend_catalog,
};

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
