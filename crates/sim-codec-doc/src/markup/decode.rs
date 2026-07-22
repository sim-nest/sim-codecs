use std::collections::BTreeMap;

use crate::document::{DocBlockKind, DocValue, decode_document};

use super::expr::format_name;
use super::{BackendId, Inline, MarkupBlock, MarkupDoc, SourceDoc, Span, SpanState};

/// Decode source text into the shared markup IR using the current lightweight
/// document parser.
pub fn decode_markup_doc(source: &str) -> MarkupDoc {
    MarkupDoc::from_doc_value(&decode_document(source))
}

impl MarkupDoc {
    pub(crate) fn from_doc_value(doc: &DocValue) -> Self {
        let title = doc
            .blocks
            .iter()
            .find(|block| block.kind == DocBlockKind::Heading && block.level == Some(1))
            .map(|block| block.text.clone());
        let blocks = doc
            .blocks
            .iter()
            .map(|block| {
                let span = Some(Span {
                    start: block.start,
                    end: block.end,
                    state: SpanState::Preserved,
                });
                match block.kind {
                    DocBlockKind::Heading => MarkupBlock::Heading {
                        level: block.level.unwrap_or(1).clamp(1, 6) as u8,
                        text: vec![Inline::Text(block.text.clone())],
                        id: None,
                        span,
                    },
                    DocBlockKind::Paragraph => MarkupBlock::Paragraph {
                        content: vec![Inline::Text(block.text.clone())],
                        span,
                    },
                }
            })
            .collect();
        Self {
            title,
            blocks,
            attrs: BTreeMap::new(),
            source: Some(SourceDoc {
                backend: BackendId(format_name(doc.format).to_owned()),
                text: doc.text.clone(),
            }),
        }
    }
}
