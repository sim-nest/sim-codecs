use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Expr, ReadPolicy, Symbol};

use crate::{ChunkOp, DocValue, chunk, decode_document};

use super::support::{cx, cx_with_general_codecs, map_string};

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
