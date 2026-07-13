use crate::{
    Inline, LatexBackend, MarkupBackend, MarkupBlock, MarkupDecodeOptions, MarkupEncodeOptions,
};

#[test]
fn article_subset_roundtrips() {
    let source = concat!(
        "\\title{Guide}\n\n",
        "Intro with \\emph{small}, \\textbf{strong}, \\texttt{code}, ",
        "\\href{https://example.test}{link}, and $x^2$.\n\n",
        "\\section{Data}\n\n",
        "\\begin{itemize}\n",
        "\\item first\n",
        "\\item second\n",
        "\\end{itemize}\n\n",
        "\\begin{tabular}{ll}\n",
        "Name & Value \\\\\n",
        "SIM & Runtime\n",
        "\\end{tabular}\n\n",
        "\\begin{verbatim}\n",
        "let x = 1;\n",
        "\\end{verbatim}\n\n",
        "\\[\n",
        "E = mc^2\n",
        "\\]\n\n",
        "\\begin{figure}\n",
        "\\includegraphics{plot.pdf}\n",
        "\\caption{Plot}\n",
        "\\end{figure}\n"
    );
    let backend = LatexBackend;
    let opts = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, fidelity) = backend.decode(source, &opts).unwrap();
    assert!(fidelity.dropped.is_empty());
    assert_eq!(doc.title.as_deref(), Some("Guide"));
    assert!(has_level_one_heading(&doc.blocks));
    assert_table_shape(&doc.blocks);
    assert!(
        doc.blocks
            .iter()
            .any(|block| matches!(block, MarkupBlock::Figure { src, .. } if src == "plot.pdf"))
    );
    assert!(doc.blocks.iter().any(
        |block| matches!(block, MarkupBlock::MathBlock { source, .. } if source.text == "\nE = mc^2\n")
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
fn input_is_preserved_not_resolved() {
    let backend = LatexBackend;
    let source = "Intro.\n\n\\input{private}\n\nOutro.\n";
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
            .any(|raw| raw.contains("\\input{private}"))
    );
    assert!(
        fidelity
            .warnings
            .iter()
            .any(|warning| warning.contains("not resolved"))
    );
    assert!(doc.blocks.iter().any(
        |block| matches!(block, MarkupBlock::Raw { text, .. } if text.contains("\\input{private}"))
    ));

    let report = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: false,
    };
    let (doc, fidelity) = backend.decode(source, &report).unwrap();
    assert!(fidelity.preserved_raw.is_empty());
    assert!(fidelity.dropped.iter().any(|loss| loss.path == "input"));
    assert!(
        doc.blocks
            .iter()
            .all(|block| !matches!(block, MarkupBlock::Raw { .. }))
    );
}

#[test]
fn math_source_preserved_byte_for_byte() {
    let backend = LatexBackend;
    let source = "Inline $\\alpha + \\beta$.\n\n\\[\\int_0^1 x\\,dx\\]\n";
    let (doc, fidelity) = backend
        .decode(
            source,
            &MarkupDecodeOptions {
                preserve_source: false,
                preserve_raw: true,
            },
        )
        .unwrap();
    assert!(fidelity.dropped.is_empty());
    assert!(doc.blocks.iter().any(|block| {
        matches!(
            block,
            MarkupBlock::Paragraph { content, .. }
                if content.iter().any(|inline| matches!(
                    inline,
                    Inline::Math(source) if source.text == "\\alpha + \\beta"
                ))
        )
    }));
    assert!(doc.blocks.iter().any(
        |block| matches!(block, MarkupBlock::MathBlock { source, .. } if source.text == "\\int_0^1 x\\,dx")
    ));
}

fn has_level_one_heading(blocks: &[MarkupBlock]) -> bool {
    blocks
        .iter()
        .any(|block| matches!(block, MarkupBlock::Heading { level: 1, .. }))
}

fn assert_table_shape(blocks: &[MarkupBlock]) {
    let table = blocks
        .iter()
        .find_map(|block| match block {
            MarkupBlock::Table { header, rows, .. } => Some((header, rows)),
            _ => None,
        })
        .expect("table");
    assert_eq!(table.0.len(), 2);
    assert_eq!(table.1.len(), 1);
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
