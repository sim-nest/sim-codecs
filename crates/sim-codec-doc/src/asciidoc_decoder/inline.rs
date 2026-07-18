use asciidork_ast as adoc;

use crate::backend::MarkupLoss;
use crate::markup::{Inline, MathSource};

use super::super::asciidoc_support::{asciidoc_id, inline_plain_text, link_target, symbol_text};
use super::AsciiDocDecoder;

impl AsciiDocDecoder<'_> {
    pub(in crate::asciidoc) fn inlines(&mut self, nodes: &adoc::InlineNodes<'_>) -> Vec<Inline> {
        let mut out = Vec::new();
        for node in nodes.iter() {
            if let Some(item) = self.inline(&node.content, node.loc) {
                out.push(item);
            }
        }
        out
    }

    fn inline(&mut self, node: &adoc::Inline<'_>, loc: adoc::SourceLocation) -> Option<Inline> {
        match node {
            adoc::Inline::Text(text) => Some(Inline::Text(text.to_string())),
            adoc::Inline::MultiCharWhitespace(_) | adoc::Inline::Newline => {
                Some(Inline::Text(" ".to_owned()))
            }
            adoc::Inline::LineBreak => Some(Inline::Text("\n".to_owned())),
            adoc::Inline::Quote(kind, children) => {
                let quote = match kind {
                    adoc::QuoteKind::Double => "\"",
                    adoc::QuoteKind::Single => "'",
                };
                Some(Inline::Text(format!(
                    "{quote}{}{quote}",
                    inline_plain_text(&self.inlines(children))
                )))
            }
            adoc::Inline::Span(kind, _, children) => match kind {
                adoc::SpanKind::Bold => Some(Inline::Strong(self.inlines(children))),
                adoc::SpanKind::Italic => Some(Inline::Emph(self.inlines(children))),
                adoc::SpanKind::LitMono | adoc::SpanKind::Mono => {
                    Some(Inline::Code(inline_plain_text(&self.inlines(children))))
                }
                adoc::SpanKind::Text => {
                    Some(Inline::Text(inline_plain_text(&self.inlines(children))))
                }
                _ => self.raw_inline_from_loc(loc, "span", "unsupported asciidoc span"),
            },
            adoc::Inline::InlinePassthru(children) => Some(Inline::Raw {
                backend: asciidoc_id(),
                text: inline_plain_text(&self.inlines(children)),
            }),
            adoc::Inline::Macro(macro_node) => self.macro_inline(macro_node, loc),
            adoc::Inline::SpecialChar(kind) => Some(Inline::Text(
                match kind {
                    adoc::SpecialCharKind::Ampersand => "&",
                    adoc::SpecialCharKind::LessThan => "<",
                    adoc::SpecialCharKind::GreaterThan => ">",
                }
                .to_owned(),
            )),
            adoc::Inline::Symbol(kind) => Some(Inline::Text(symbol_text(*kind).to_owned())),
            adoc::Inline::CurlyQuote(kind) => Some(Inline::Text(
                match kind {
                    adoc::CurlyKind::LeftDouble | adoc::CurlyKind::RightDouble => "\"",
                    _ => "'",
                }
                .to_owned(),
            )),
            adoc::Inline::LineComment(text) => self.raw_inline(
                text.to_string(),
                "comment",
                "asciidoc line comment is preserved raw",
            ),
            adoc::Inline::Discarded
            | adoc::Inline::CalloutNum(_)
            | adoc::Inline::CalloutTuck(_)
            | adoc::Inline::InlineAnchor(_)
            | adoc::Inline::BiblioAnchor(_)
            | adoc::Inline::IndexTerm(_)
            | adoc::Inline::SpacedDashes(_, _) => None,
        }
    }

    fn macro_inline(
        &mut self,
        macro_node: &adoc::MacroNode<'_>,
        loc: adoc::SourceLocation,
    ) -> Option<Inline> {
        match macro_node {
            adoc::MacroNode::Link {
                scheme,
                target,
                attrs,
                ..
            } => {
                let target = link_target(*scheme, target);
                let label = attrs
                    .as_ref()
                    .and_then(|attrs| adoc::AttrData::positional_at(attrs, 0))
                    .map(|nodes| self.inlines(nodes))
                    .unwrap_or_else(|| vec![Inline::Text(target.clone())]);
                Some(Inline::Link { label, target })
            }
            adoc::MacroNode::Mailto {
                address, linktext, ..
            } => {
                let target = format!("mailto:{}", address.src);
                let label = linktext
                    .as_ref()
                    .map(|nodes| self.inlines(nodes))
                    .unwrap_or_else(|| vec![Inline::Text(address.src.to_string())]);
                Some(Inline::Link { label, target })
            }
            adoc::MacroNode::Xref {
                target, linktext, ..
            } => {
                let target = format!("#{}", target.src);
                let label = linktext
                    .as_ref()
                    .map(|nodes| self.inlines(nodes))
                    .unwrap_or_else(|| vec![Inline::Text(target.clone())]);
                Some(Inline::Link { label, target })
            }
            adoc::MacroNode::Plugin(plugin)
                if matches!(plugin.name.as_str(), "stem" | "latexmath" | "asciimath") =>
            {
                let text = plugin
                    .target
                    .as_ref()
                    .map(|target| target.src.to_string())
                    .unwrap_or_else(|| plugin.source.src.to_string());
                Some(Inline::Math(MathSource {
                    notation: "asciidoc".to_owned(),
                    text,
                }))
            }
            adoc::MacroNode::InlineImage { target, .. } => self.raw_inline(
                target.src.to_string(),
                "inline-image",
                "asciidoc inline image is preserved raw",
            ),
            adoc::MacroNode::Plugin(plugin) => self.raw_inline(
                plugin.source.src.to_string(),
                plugin.name.as_str(),
                "unsupported asciidoc macro is preserved raw",
            ),
            _ => self.raw_inline_from_loc(loc, "macro", "unsupported asciidoc macro"),
        }
    }

    fn raw_inline_from_loc(
        &mut self,
        loc: adoc::SourceLocation,
        path: &str,
        reason: &str,
    ) -> Option<Inline> {
        let text = self
            .input
            .get(usize::try_from(loc.start).ok()?..usize::try_from(loc.end).ok()?)
            .unwrap_or_default()
            .to_owned();
        self.raw_inline(text, path, reason)
    }

    fn raw_inline(&mut self, text: String, path: &str, reason: &str) -> Option<Inline> {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(text.clone());
            Some(Inline::Raw {
                backend: asciidoc_id(),
                text,
            })
        } else {
            self.fidelity.dropped.push(MarkupLoss {
                path: path.to_owned(),
                reason: reason.to_owned(),
            });
            None
        }
    }
}
