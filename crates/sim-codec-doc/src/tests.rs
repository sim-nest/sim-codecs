use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Expr, NumberLiteral, ReadPolicy, Symbol};

use crate::{
    BackendId, BackendRegistry, ChunkOp, DocValue, Inline, MarkdownBackend, MarkupBackend,
    MarkupBlock, MarkupDecodeOptions, MarkupDoc, MarkupEncodeOptions, MarkupError, MarkupFidelity,
    MathSource, SourceDoc, Span, chunk, decode_document, decode_markup_doc, install_doc_codec,
    install_markup_codecs, markup_codec_symbol,
};

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_test_support::core_cx();
    sim_test_support::register_f64_number_domain(&mut cx);
    install_doc_codec(&mut cx).unwrap();
    cx
}

fn cx_with_general_codecs() -> sim_kernel::Cx {
    let mut cx = cx();
    let json = sim_codec_json::JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&json).unwrap();
    let lisp = sim_codec_lisp::LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    cx
}

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
    for symbol in [
        Symbol::qualified("doc", "chunk-fixed"),
        Symbol::qualified("doc", "chunk-recursive"),
        Symbol::qualified("doc", "chunk-heading"),
    ] {
        assert!(
            cx.registry().function_by_symbol(&symbol).is_some(),
            "missing {symbol}"
        );
    }
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
fn decode_document_still_chunks_headings() {
    let doc = decode_document("# Guide\n\nAlpha beta.\n\n## Detail\n\nGamma.\n");
    assert_eq!(doc.blocks.len(), 4);
    assert_eq!(doc.blocks[0].text, "Guide");
    assert_eq!(doc.blocks[0].start, 0);
    assert_eq!(doc.blocks[0].end, 7);
    assert_eq!(doc.blocks[1].text, "Alpha beta.");
    assert_eq!(doc.blocks[1].start, 9);
    assert_eq!(doc.blocks[1].end, 20);
    assert_eq!(doc.blocks[1].heading_path, vec!["Guide"]);
    assert_eq!(doc.blocks[3].heading_path, vec!["Guide", "Detail"]);
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

#[test]
fn fixed_chunks_preserve_source_offsets() {
    let doc = decode_document("abcdef");
    let chunks = chunk(&doc, ChunkOp::Fixed(2));
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| (chunk.text.as_str(), chunk.start, chunk.end))
            .collect::<Vec<_>>(),
        vec![("ab", 0, 2), ("cd", 2, 4), ("ef", 4, 6)]
    );
}

#[test]
fn recursive_chunks_prefer_paragraphs_and_split_large_blocks() {
    let doc = decode_document("short\n\nlonger-block");
    let chunks = chunk(&doc, ChunkOp::Recursive { max: 6 });
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| (chunk.text.as_str(), chunk.start, chunk.end))
            .collect::<Vec<_>>(),
        vec![("short", 0, 5), ("longer", 7, 13), ("-block", 13, 19)]
    );
}

#[test]
fn heading_chunks_attach_current_heading_path() {
    let doc = decode_document("# Guide\n\nAlpha beta.\n\n## Detail\n\nGamma.\n");
    let chunks = chunk(&doc, ChunkOp::Heading);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].text, "Alpha beta.");
    assert_eq!(chunks[0].heading_path, vec!["Guide"]);
    assert_eq!(chunks[1].text, "Gamma.");
    assert_eq!(chunks[1].heading_path, vec!["Guide", "Detail"]);
}

#[test]
fn chunk_functions_return_chunk_maps() {
    let mut cx = cx();
    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "doc"),
        Input::Text("abcdef".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    let doc = cx.factory().expr(decoded).unwrap();
    let size = cx
        .factory()
        .number_literal(Symbol::qualified("numbers", "f64"), "3".to_owned())
        .unwrap();
    let function = cx
        .registry()
        .function_by_symbol(&Symbol::qualified("doc", "chunk-fixed"))
        .unwrap()
        .clone();
    let value = cx
        .call_value(function, sim_kernel::Args::new(vec![doc, size]))
        .unwrap();
    let Expr::List(items) = value.object().as_expr(&mut cx).unwrap() else {
        panic!("expected list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(map_string(&items[0], "text"), Some("abc"));
}

#[test]
fn chunk_values_roundtrip_through_lisp_and_json_codecs() {
    let mut cx = cx_with_general_codecs();
    let chunks = chunk(
        &decode_document("# Guide\n\nAlpha beta.\n"),
        ChunkOp::Heading,
    );
    let expr = Expr::List(chunks.into_iter().map(|chunk| chunk.as_expr()).collect());
    for codec in [
        Symbol::qualified("codec", "json"),
        Symbol::qualified("codec", "lisp"),
    ] {
        let output = encode_with_codec(&mut cx, &codec, &expr, EncodeOptions::default()).unwrap();
        let input = match output {
            sim_codec::Output::Text(text) => Input::Text(text),
            sim_codec::Output::Bytes(bytes) => Input::Bytes(bytes),
        };
        let decoded = decode_with_codec(&mut cx, &codec, input, ReadPolicy::default()).unwrap();
        assert!(decoded.canonical_eq(&expr), "{codec} did not round-trip");
    }
}

#[test]
fn invalid_document_input_is_rejected() {
    let err = DocValue::from_expr(&Expr::List(Vec::new())).unwrap_err();
    assert!(err.to_string().contains("document"));
}

#[test]
fn zero_size_function_input_is_rejected() {
    let mut cx = cx();
    let doc = cx.factory().expr(decode_document("abc").as_expr()).unwrap();
    let size = cx
        .factory()
        .number_literal(Symbol::qualified("numbers", "f64"), "0".to_owned())
        .unwrap();
    let function = cx
        .registry()
        .function_by_symbol(&Symbol::qualified("doc", "chunk-recursive"))
        .unwrap()
        .clone();
    let err = cx
        .call_value(function, sim_kernel::Args::new(vec![doc, size]))
        .unwrap_err();
    assert!(err.to_string().contains("greater than zero"));
}

fn map_symbol(expr: &Expr, field: &str) -> Option<Symbol> {
    match map_field(expr, field)? {
        Expr::Symbol(symbol) => Some(symbol.clone()),
        _ => None,
    }
}

fn map_string<'a>(expr: &'a Expr, field: &str) -> Option<&'a str> {
    match map_field(expr, field)? {
        Expr::String(text) => Some(text),
        _ => None,
    }
}

fn map_field<'a>(expr: &'a Expr, field: &str) -> Option<&'a Expr> {
    let Expr::Map(entries) = expr else {
        return None;
    };
    entries.iter().find_map(|(key, value)| {
        matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == field).then_some(value)
    })
}

fn blocks_without_spans(blocks: &[MarkupBlock]) -> Vec<MarkupBlock> {
    blocks.iter().cloned().map(block_without_span).collect()
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
            blocks: blocks_without_spans(&blocks),
            span: None,
        },
        MarkupBlock::List { ordered, items, .. } => MarkupBlock::List {
            ordered,
            items: items
                .into_iter()
                .map(|item| blocks_without_spans(&item))
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

fn _number(value: usize) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}

#[derive(Clone)]
struct TestBackend {
    id: BackendId,
}

impl TestBackend {
    fn new(id: &str) -> Self {
        Self {
            id: BackendId::new(id),
        }
    }
}

impl MarkupBackend for TestBackend {
    fn id(&self) -> BackendId {
        self.id.clone()
    }

    fn decode(
        &self,
        input: &str,
        _opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        Ok((decode_markup_doc(input), MarkupFidelity::exact(self.id())))
    }

    fn encode(
        &self,
        doc: &MarkupDoc,
        _opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        Ok((doc.to_source_text(), MarkupFidelity::exact(self.id())))
    }
}
