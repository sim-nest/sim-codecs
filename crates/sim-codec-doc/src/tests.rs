use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Expr, ReadPolicy, Symbol};

use crate::{
    BackendId, BackendRegistry, Inline, MarkdownBackend, MarkupBackend, MarkupBlock,
    MarkupDecodeOptions, MarkupDoc, MarkupEncodeOptions, MarkupError, MathSource, SourceDoc, Span,
    TypstBackend, install_markup_codecs, markup_codec_symbol,
};

mod chunk;
mod support;

use support::{
    TestBackend, blocks_without_spans, call_report, cx, map_field, map_string, map_symbol,
};

#[test]
fn doc_codec_registers_codec_and_chunk_functions() {
    let cx = cx();
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "doc"))
            .is_some()
    );
    assert!(
        cx.registry()
            .codec_by_symbol(&markup_codec_symbol(&BackendId::new("markdown")))
            .is_some()
    );
    assert!(
        cx.registry()
            .codec_by_symbol(&markup_codec_symbol(&BackendId::new("asciidoc")))
            .is_some()
    );
    assert!(
        cx.registry()
            .codec_by_symbol(&markup_codec_symbol(&BackendId::new("latex")))
            .is_some()
    );
    assert!(
        cx.registry()
            .codec_by_symbol(&markup_codec_symbol(&BackendId::new("typst")))
            .is_some()
    );
    for symbol in [
        Symbol::qualified("doc", "chunk-fixed"),
        Symbol::qualified("doc", "chunk-recursive"),
        Symbol::qualified("doc", "chunk-heading"),
        Symbol::qualified("doc", "backend-catalog"),
        Symbol::qualified("doc", "markdown-to-typst"),
    ] {
        assert!(
            cx.registry().function_by_symbol(&symbol).is_some(),
            "missing {symbol}"
        );
    }
}

#[test]
fn cookbook_catalog_and_transcode_functions_run() {
    let mut cx = cx();
    let catalog = call_report(&mut cx, Symbol::qualified("doc", "backend-catalog"));
    assert_eq!(
        map_symbol(&catalog, "kind"),
        Some(Symbol::qualified("doc", "backend-catalog"))
    );

    let transcode = call_report(&mut cx, Symbol::qualified("doc", "markdown-to-typst"));
    assert_eq!(map_string(&transcode, "to"), Some("typst"));
}

#[test]
fn backend_registry_is_deterministic() {
    let mut registry = BackendRegistry::new();
    registry.register(TestBackend::new("zeta"));
    registry.register(TestBackend::new("alpha"));
    registry.register(TestBackend::new("markdown"));
    assert_eq!(
        registry.ids(),
        vec![
            BackendId::new("alpha"),
            BackendId::new("markdown"),
            BackendId::new("zeta"),
        ]
    );
}

#[test]
fn unknown_backend_fails_closed() {
    let registry = BackendRegistry::new();
    match registry.backend(&BackendId::new("missing")) {
        Err(MarkupError::UnknownBackend(id)) => assert_eq!(id, BackendId::new("missing")),
        _ => panic!("expected unknown backend"),
    }

    let mut cx = sim_test_support::core_cx();
    let err = decode_with_codec(
        &mut cx,
        &markup_codec_symbol(&BackendId::new("missing")),
        Input::Text("# Guide".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("codec/markup/missing"));
}

#[test]
fn markup_codecs_get_distinct_symbols() {
    let mut cx = sim_test_support::core_cx();
    let mut registry = BackendRegistry::new();
    registry.register(TestBackend::new("alpha"));
    registry.register(TestBackend::new("beta"));
    install_markup_codecs(&mut cx, registry).unwrap();

    let alpha = markup_codec_symbol(&BackendId::new("alpha"));
    let beta = markup_codec_symbol(&BackendId::new("beta"));
    assert_ne!(alpha, beta);
    assert!(cx.registry().codec_by_symbol(&alpha).is_some());
    assert!(cx.registry().codec_by_symbol(&beta).is_some());
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "doc"))
            .is_none()
    );
}

#[test]
fn markup_codec_roundtrips_markup_and_rejects_non_markup() {
    let mut cx = sim_test_support::core_cx();
    let mut registry = BackendRegistry::new();
    registry.register(TestBackend::new("alpha"));
    install_markup_codecs(&mut cx, registry).unwrap();
    let codec = markup_codec_symbol(&BackendId::new("alpha"));

    let expr = decode_with_codec(
        &mut cx,
        &codec,
        Input::Text("# Guide\n\nAlpha beta.\n".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(map_symbol(&expr, "kind"), Some(Symbol::new("markup-doc")));
    let output = encode_with_codec(&mut cx, &codec, &expr, EncodeOptions::default()).unwrap();
    assert_eq!(output.into_text().unwrap(), "# Guide\n\nAlpha beta.\n");

    let err = encode_with_codec(
        &mut cx,
        &codec,
        &Expr::String("not a markup value".to_owned()),
        EncodeOptions::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("invalid markup document"));
}

#[test]
fn markdown_roundtrips_semantically() {
    let source = "# Guide\n\nA *small* **sample** with `code`, [SIM](https://example.test), and $x^2$.\n\n> Quoted\n\n```rust\nfn main() {}\n```\n\n$$\ny = x^2\n$$\n";
    let backend = MarkdownBackend;
    let opts = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, fidelity) = backend.decode(source, &opts).unwrap();
    assert!(fidelity.dropped.is_empty());
    assert_eq!(doc.title.as_deref(), Some("Guide"));
    assert!(
        doc.blocks
            .iter()
            .any(|block| matches!(block, MarkupBlock::Quote { .. }))
    );
    assert!(doc.blocks.iter().any(
        |block| matches!(block, MarkupBlock::MathBlock { source, .. } if source.notation == "tex")
    ));

    let (encoded, encode_fidelity) = backend
        .encode(&doc, &MarkupEncodeOptions::default())
        .unwrap();
    assert!(encode_fidelity.dropped.is_empty());
    let (decoded, _) = backend.decode(&encoded, &opts).unwrap();
    assert_eq!(
        blocks_without_spans(&decoded.blocks),
        blocks_without_spans(&doc.blocks)
    );
}

#[test]
fn raw_html_is_preserved_or_reported() {
    let backend = MarkdownBackend;
    let source = "Text with <span data-x=\"1\">raw</span>.\n";
    let preserve = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, fidelity) = backend.decode(source, &preserve).unwrap();
    assert!(fidelity.dropped.is_empty());
    assert!(
        fidelity
            .preserved_raw
            .iter()
            .any(|raw| raw.contains("<span"))
    );
    assert!(
        matches!(&doc.blocks[0], MarkupBlock::Paragraph { content, .. } if content.iter().any(|inline| matches!(inline, Inline::Raw { text, .. } if text.contains("<span"))))
    );

    let report = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: false,
    };
    let (reported, fidelity) = backend.decode(source, &report).unwrap();
    assert!(fidelity.preserved_raw.is_empty());
    assert!(fidelity.dropped.iter().any(|loss| loss.path == "html"));
    assert!(
        matches!(&reported.blocks[0], MarkupBlock::Paragraph { content, .. } if content.iter().all(|inline| !matches!(inline, Inline::Raw { .. })))
    );
}

#[test]
fn markdown_tables_and_task_markers_survive() {
    let backend = MarkdownBackend;
    let source = "| Task | Done |\n| --- | --- |\n| Ship | yes |\n\n- [x] parser\n- [ ] writer\n";
    let (doc, _) = backend
        .decode(
            source,
            &MarkupDecodeOptions {
                preserve_source: false,
                preserve_raw: true,
            },
        )
        .unwrap();
    let MarkupBlock::Table { header, rows, .. } = &doc.blocks[0] else {
        panic!("expected table");
    };
    assert_eq!(header.len(), 2);
    assert_eq!(rows.len(), 1);
    let MarkupBlock::List { items, .. } = &doc.blocks[1] else {
        panic!("expected task list");
    };
    assert!(
        matches!(&items[0][0], MarkupBlock::Paragraph { content, .. } if matches!(&content[0], Inline::Raw { text, .. } if text == "[x] "))
    );
    assert!(
        matches!(&items[1][0], MarkupBlock::Paragraph { content, .. } if matches!(&content[0], Inline::Raw { text, .. } if text == "[ ] "))
    );

    let (encoded, _) = backend
        .encode(&doc, &MarkupEncodeOptions::default())
        .unwrap();
    assert!(encoded.contains("| Task | Done |"));
    assert!(encoded.contains("- [x] parser"));
    assert!(encoded.contains("- [ ] writer"));
}

#[test]
fn typst_roundtrips_semantically() {
    let source = concat!(
        "= Guide\n\n",
        "A _small_ *sample* with `code`, https://example.test, and $x^2$.\n\n",
        "$ y = x^2 $\n\n",
        "- first\n",
        "- second\n\n",
        "#table(columns: 2,\n",
        "  [Name],\n",
        "  [Value],\n",
        "  [SIM],\n",
        "  [Runtime]\n",
        ")\n\n",
        "#figure(image(\"plot.svg\"), caption: [Plot])\n"
    );
    let backend = TypstBackend;
    let opts = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, fidelity) = backend.decode(source, &opts).unwrap();
    assert!(fidelity.dropped.is_empty());
    assert_eq!(doc.title.as_deref(), Some("Guide"));
    assert!(
        doc.blocks
            .iter()
            .any(|block| matches!(block, MarkupBlock::Table { .. }))
    );
    assert!(
        doc.blocks
            .iter()
            .any(|block| matches!(block, MarkupBlock::Figure { src, .. } if src == "plot.svg"))
    );
    assert!(doc.blocks.iter().any(
        |block| matches!(block, MarkupBlock::MathBlock { source, .. } if source.notation == "typst")
    ));

    let (encoded, encode_fidelity) = backend
        .encode(&doc, &MarkupEncodeOptions::default())
        .unwrap();
    assert!(encode_fidelity.dropped.is_empty());
    let (decoded, _) = backend.decode(&encoded, &opts).unwrap();
    assert_eq!(
        blocks_without_spans(&decoded.blocks),
        blocks_without_spans(&doc.blocks)
    );
}

#[test]
fn typst_functions_are_not_executed() {
    let backend = TypstBackend;
    let source = "#include \"secret.typ\"\n\n#external(data)\n";
    let preserve = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, fidelity) = backend.decode(source, &preserve).unwrap();
    assert!(fidelity.dropped.is_empty());
    assert!(
        fidelity
            .preserved_raw
            .iter()
            .any(|raw| raw.contains("include"))
    );
    assert!(
        doc.blocks.iter().any(
            |block| matches!(block, MarkupBlock::Raw { text, .. } if text.contains("include"))
        )
    );
    assert!(doc.blocks.iter().any(
        |block| matches!(block, MarkupBlock::Raw { text, span: Some(_), .. } if text.contains("external"))
    ));

    let report = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: false,
    };
    let (_doc, fidelity) = backend.decode(source, &report).unwrap();
    assert!(fidelity.preserved_raw.is_empty());
    assert!(fidelity.dropped.iter().any(|loss| loss.path == "include"));
    assert!(
        fidelity
            .dropped
            .iter()
            .any(|loss| loss.path == "inline" || loss.path == "external")
    );
}

#[test]
fn markup_doc_roundtrips_as_expr() {
    let mut attrs = std::collections::BTreeMap::new();
    attrs.insert("audience".to_owned(), Expr::String("builder".to_owned()));
    let doc = MarkupDoc {
        title: Some("Guide".to_owned()),
        attrs,
        source: Some(SourceDoc {
            backend: BackendId("markdown".to_owned()),
            text: "# Guide\n\nAlpha beta.\n".to_owned(),
        }),
        blocks: vec![
            MarkupBlock::Heading {
                level: 1,
                text: vec![Inline::Text("Guide".to_owned())],
                id: Some("guide".to_owned()),
                span: Some(Span {
                    start: 0,
                    end: 7,
                    state: crate::SpanState::Preserved,
                }),
            },
            MarkupBlock::Paragraph {
                content: vec![
                    Inline::Text("Alpha ".to_owned()),
                    Inline::Strong(vec![Inline::Text("beta".to_owned())]),
                    Inline::Text(".".to_owned()),
                ],
                span: Some(Span {
                    start: 9,
                    end: 20,
                    state: crate::SpanState::Preserved,
                }),
            },
            MarkupBlock::CodeBlock {
                lang: Some("rust".to_owned()),
                code: "fn main() {}".to_owned(),
                span: None,
            },
            MarkupBlock::Table {
                header: vec![vec![Inline::Text("Name".to_owned())]],
                rows: vec![vec![vec![Inline::Text("SIM".to_owned())]]],
                span: None,
            },
            MarkupBlock::MathBlock {
                source: MathSource {
                    notation: "tex".to_owned(),
                    text: "x^2".to_owned(),
                },
                span: None,
            },
        ],
    };

    let decoded = MarkupDoc::from_expr(&doc.as_expr()).unwrap();
    assert_eq!(decoded, doc);
}

#[test]
fn codec_decodes_text_to_markup_doc_expr_and_encodes_source_text() {
    let mut cx = cx();
    let source = "# Guide\n\nAlpha beta.\n";
    let expr = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "doc"),
        Input::Text(source.to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(map_symbol(&expr, "kind"), Some(Symbol::new("markup-doc")));
    assert_eq!(map_string(&expr, "title"), Some("Guide"));
    assert_eq!(
        map_string(map_field(&expr, "source").unwrap(), "text"),
        Some(source)
    );
    let output = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "doc"),
        &expr,
        EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(output.into_text().unwrap(), source);
}
