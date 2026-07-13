use crate::{
    AsciiDocBackend, MarkupBackend, MarkupBlock, MarkupDecodeOptions, MarkupEncodeOptions,
};

#[test]
fn sections_and_tables_roundtrip() {
    let source = concat!(
        "= Guide\n\n",
        "Intro with _small_ and *strong* text.\n\n",
        "== Data\n\n",
        "[options=\"header\"]\n",
        "|===\n",
        "| Name | Value\n",
        "| SIM | Runtime\n",
        "|===\n\n",
        "image::plot.svg[Plot]\n"
    );
    let backend = AsciiDocBackend;
    let opts = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: true,
    };
    let (doc, fidelity) = backend.decode(source, &opts).unwrap();
    assert!(fidelity.dropped.is_empty());
    assert_eq!(doc.title.as_deref(), Some("Guide"));
    assert!(has_level_two_heading(&doc.blocks));
    assert_table_shape(&doc.blocks);
    assert!(
        doc.blocks
            .iter()
            .any(|block| matches!(block, MarkupBlock::Figure { src, .. } if src == "plot.svg"))
    );

    let (encoded, encode_fidelity) = backend
        .encode(&doc, &MarkupEncodeOptions::default())
        .unwrap();
    assert!(encode_fidelity.dropped.is_empty());
    let (encoded_again, _) = backend
        .encode(&doc, &MarkupEncodeOptions::default())
        .unwrap();
    assert_eq!(encoded, encoded_again);
    let (decoded, _) = backend.decode(&encoded, &opts).unwrap();
    assert_eq!(decoded.title, doc.title);
    assert!(has_level_two_heading(&decoded.blocks));
    assert_table_shape(&decoded.blocks);
}

#[test]
fn include_macro_is_not_resolved() {
    let backend = AsciiDocBackend;
    let source = "Intro.\n\ninclude::secret[]\n\nOutro.\n";
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
            .any(|raw| raw.contains("include::secret"))
    );
    assert!(
        fidelity
            .warnings
            .iter()
            .any(|warning| warning.contains("include"))
    );
    assert!(doc.blocks.iter().any(
        |block| matches!(block, MarkupBlock::Raw { text, .. } if text.contains("include::secret"))
    ));

    let report = MarkupDecodeOptions {
        preserve_source: false,
        preserve_raw: false,
    };
    let (doc, fidelity) = backend.decode(source, &report).unwrap();
    assert!(fidelity.preserved_raw.is_empty());
    assert!(fidelity.dropped.iter().any(|loss| loss.path == "include"));
    assert!(
        doc.blocks
            .iter()
            .all(|block| !matches!(block, MarkupBlock::Raw { .. }))
    );
}

fn has_level_two_heading(blocks: &[MarkupBlock]) -> bool {
    blocks
        .iter()
        .any(|block| matches!(block, MarkupBlock::Heading { level: 2, .. }))
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
