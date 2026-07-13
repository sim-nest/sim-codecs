use crate::{Inline, MarkupBlock, MarkupDoc, MarkupEdit, Span, SpanState, apply_edit, invert_edit};

#[test]
fn invert_edit_is_involutive() {
    let edits = vec![
        MarkupEdit::SetTitle {
            old: Some("Old".to_owned()),
            new: Some("New".to_owned()),
        },
        MarkupEdit::InsertBlock {
            index: 1,
            block: paragraph("Inserted", None),
        },
        MarkupEdit::ReplaceBlock {
            index: 0,
            old: paragraph("Old", None),
            new: paragraph("New", None),
        },
        MarkupEdit::DeleteBlock {
            index: 0,
            old: paragraph("Old", None),
        },
        MarkupEdit::SetInlineText {
            block: 0,
            path: vec![1, 0],
            old: "old".to_owned(),
            new: "new".to_owned(),
        },
    ];

    for edit in edits {
        assert_eq!(invert_edit(&invert_edit(&edit)), edit);
    }
}

#[test]
fn apply_then_inverse_restores_document() {
    let original = MarkupDoc {
        title: Some("Guide".to_owned()),
        attrs: Default::default(),
        source: None,
        blocks: vec![paragraph("Alpha beta.", None)],
    };
    let edit = MarkupEdit::SetInlineText {
        block: 0,
        path: vec![0],
        old: "Alpha beta.".to_owned(),
        new: "Gamma.".to_owned(),
    };
    let mut edited = original.clone();

    apply_edit(&mut edited, &edit).unwrap();
    apply_edit(&mut edited, &invert_edit(&edit)).unwrap();

    assert_eq!(edited, original);
}

#[test]
fn dirty_spans_stay_local() {
    let mut doc = MarkupDoc {
        title: None,
        attrs: Default::default(),
        source: None,
        blocks: vec![
            paragraph(
                "Alpha",
                Some(Span {
                    start: 0,
                    end: 5,
                    state: SpanState::Preserved,
                }),
            ),
            paragraph(
                "Beta",
                Some(Span {
                    start: 7,
                    end: 11,
                    state: SpanState::Preserved,
                }),
            ),
        ],
    };
    let edit = MarkupEdit::SetInlineText {
        block: 0,
        path: vec![0],
        old: "Alpha".to_owned(),
        new: "Gamma".to_owned(),
    };

    apply_edit(&mut doc, &edit).unwrap();

    assert_eq!(block_span_state(&doc.blocks[0]), Some(&SpanState::Dirty));
    assert_eq!(
        block_span_state(&doc.blocks[1]),
        Some(&SpanState::Preserved)
    );
}

#[test]
fn edit_roundtrips_through_value_expr() {
    let edits = vec![
        MarkupEdit::SetTitle {
            old: None,
            new: Some("Guide".to_owned()),
        },
        MarkupEdit::InsertBlock {
            index: 0,
            block: paragraph("Inserted", None),
        },
        MarkupEdit::ReplaceBlock {
            index: 0,
            old: paragraph("Old", None),
            new: paragraph("New", None),
        },
        MarkupEdit::DeleteBlock {
            index: 0,
            old: paragraph("Old", None),
        },
        MarkupEdit::SetInlineText {
            block: 0,
            path: vec![0],
            old: "Old".to_owned(),
            new: "New".to_owned(),
        },
    ];

    for edit in edits {
        let decoded = MarkupEdit::from_expr(&edit.as_expr()).unwrap();
        assert_eq!(decoded, edit);
    }
}

fn paragraph(text: &str, span: Option<Span>) -> MarkupBlock {
    MarkupBlock::Paragraph {
        content: vec![Inline::Text(text.to_owned())],
        span,
    }
}

fn block_span_state(block: &MarkupBlock) -> Option<&SpanState> {
    match block {
        MarkupBlock::Paragraph {
            span: Some(span), ..
        } => Some(&span.state),
        _ => None,
    }
}
