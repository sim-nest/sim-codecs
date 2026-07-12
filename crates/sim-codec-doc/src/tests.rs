use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Expr, NumberLiteral, ReadPolicy, Symbol};

use crate::{
    BackendId, ChunkOp, DocValue, Inline, MarkupBlock, MarkupDoc, MathSource, SourceDoc, Span,
    chunk, decode_document, install_doc_codec,
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
                span: Some(Span { start: 0, end: 7 }),
            },
            MarkupBlock::Paragraph {
                content: vec![
                    Inline::Text("Alpha ".to_owned()),
                    Inline::Strong(vec![Inline::Text("beta".to_owned())]),
                    Inline::Text(".".to_owned()),
                ],
                span: Some(Span { start: 9, end: 20 }),
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

fn _number(value: usize) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}
