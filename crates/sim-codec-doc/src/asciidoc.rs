//! AsciiDoc backend implementation over `asciidork-parser`.

use std::collections::BTreeMap;

use asciidork_parser::prelude::{Bump, Parser, SourceFile};

use crate::MarkupFidelity;
use crate::backend::{MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions, MarkupError};
use crate::markup::{BackendId, MarkupDoc, SourceDoc};

#[path = "asciidoc_decoder.rs"]
mod asciidoc_decoder;
#[path = "asciidoc_support.rs"]
mod asciidoc_support;
#[path = "asciidoc_writer.rs"]
mod asciidoc_writer;

use asciidoc_decoder::AsciiDocDecoder;
use asciidoc_support::{asciidoc_id, inline_plain_text, mask_directives, raw_directives};
use asciidoc_writer::AsciiDocEncoder;

/// Safe AsciiDoc markup backend.
#[derive(Clone, Debug, Default)]
pub struct AsciiDocBackend;

impl MarkupBackend for AsciiDocBackend {
    fn id(&self) -> BackendId {
        asciidoc_id()
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        let directives = raw_directives(input);
        let masked = mask_directives(input, &directives);
        let parse_input = masked.as_ref();
        let bump = Bump::new();
        let parser = Parser::from_str(parse_input, SourceFile::Tmp, &bump);
        let result = parser.parse().map_err(|diagnostics| {
            MarkupError::Decode(
                diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;

        let mut decoder = AsciiDocDecoder::new(input, opts);
        decoder.fidelity.warnings.extend(
            result
                .warnings
                .into_iter()
                .map(|diagnostic| diagnostic.message),
        );
        decoder.record_directives(&directives);
        let title = result
            .document
            .title()
            .map(|title| inline_plain_text(&decoder.inlines(&title.main)));
        let parsed_blocks = decoder.blocks_from_content(&result.document.content);
        let blocks = decoder.merge_directives(parsed_blocks);
        let source = opts.preserve_source.then(|| SourceDoc {
            backend: asciidoc_id(),
            text: input.to_owned(),
        });

        Ok((
            MarkupDoc {
                title,
                blocks,
                attrs: BTreeMap::new(),
                source,
            },
            decoder.fidelity,
        ))
    }

    fn encode(
        &self,
        doc: &MarkupDoc,
        opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        let mut encoder = AsciiDocEncoder::new(opts);
        let source = encoder.write_doc(doc);
        if opts.fail_on_loss && !encoder.fidelity().dropped.is_empty() {
            return Err(MarkupError::Encode(format!(
                "asciidoc encode dropped {} unsupported fragment(s)",
                encoder.fidelity().dropped.len()
            )));
        }
        Ok((source, encoder.into_fidelity()))
    }
}
