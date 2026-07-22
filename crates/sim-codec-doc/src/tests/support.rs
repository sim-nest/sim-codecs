use sim_kernel::{Args, Expr, NumberLiteral, Symbol};
pub(super) use sim_value::access::field as map_field;

use crate::{
    BackendId, MarkupBackend, MarkupBlock, MarkupDecodeOptions, MarkupDoc, MarkupEncodeOptions,
    MarkupError, MarkupFidelity, decode_markup_doc, install_doc_codec,
};

pub(super) fn cx() -> sim_kernel::Cx {
    let mut cx = sim_test_support::core_cx();
    sim_test_support::register_f64_number_domain(&mut cx);
    install_doc_codec(&mut cx).unwrap();
    cx
}

pub(super) fn cx_with_general_codecs() -> sim_kernel::Cx {
    let mut cx = cx();
    let json = sim_codec_json::JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&json).unwrap();
    let lisp = sim_codec_lisp::LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    cx
}

pub(super) fn call_report(cx: &mut sim_kernel::Cx, symbol: Symbol) -> Expr {
    let value = cx.registry().function_by_symbol(&symbol).unwrap().clone();
    let callable = value.object().as_callable().unwrap();
    let value = callable.call(cx, Args::new(Vec::new())).unwrap();
    value.object().as_expr(cx).unwrap()
}

pub(super) fn map_symbol(expr: &Expr, field: &str) -> Option<Symbol> {
    match map_field(expr, field)? {
        Expr::Symbol(symbol) => Some(symbol.clone()),
        _ => None,
    }
}

pub(super) fn map_string<'a>(expr: &'a Expr, field: &str) -> Option<&'a str> {
    match map_field(expr, field)? {
        Expr::String(text) => Some(text),
        _ => None,
    }
}

pub(super) fn blocks_without_spans(blocks: &[MarkupBlock]) -> Vec<MarkupBlock> {
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

#[derive(Clone)]
pub(super) struct TestBackend {
    id: BackendId,
}

impl TestBackend {
    pub(super) fn new(id: &str) -> Self {
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

#[allow(dead_code)]
pub(super) fn _number(value: usize) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}
