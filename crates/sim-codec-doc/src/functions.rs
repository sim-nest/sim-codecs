//! Document cookbook operations exposed as kernel callables.

use std::any::Any;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, NumberLiteral, Object, ObjectCompat, Result, Symbol,
    Value,
};

use crate::backend::{MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions};
use crate::catalog::{BackendStatus, backend_catalog};
use crate::document::{ChunkOp, DocValue, chunk};
use crate::{MarkdownBackend, TypstBackend};

pub const CHUNK_FUNCTIONS: &[DocChunkFunctionKind] = &[
    DocChunkFunctionKind::Fixed,
    DocChunkFunctionKind::Recursive,
    DocChunkFunctionKind::Heading,
];

pub const CATALOG_FUNCTIONS: &[DocCatalogFunctionKind] = &[
    DocCatalogFunctionKind::BackendCatalog,
    DocCatalogFunctionKind::MarkdownToTypst,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocChunkFunctionKind {
    Fixed,
    Recursive,
    Heading,
}

pub struct DocChunkFunction {
    kind: DocChunkFunctionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocCatalogFunctionKind {
    BackendCatalog,
    MarkdownToTypst,
}

pub struct DocCatalogFunction {
    kind: DocCatalogFunctionKind,
}

impl DocChunkFunction {
    pub fn new(kind: DocChunkFunctionKind) -> Self {
        Self { kind }
    }
}

impl DocCatalogFunction {
    pub fn new(kind: DocCatalogFunctionKind) -> Self {
        Self { kind }
    }
}

impl Callable for DocChunkFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let values = args.values();
        let doc = doc_arg(cx, values.first(), self.kind)?;
        let chunks = match self.kind {
            DocChunkFunctionKind::Fixed => {
                let max = size_arg(cx, values.get(1), "doc/chunk-fixed requires a size")?;
                chunk(&doc, ChunkOp::Fixed(max))
            }
            DocChunkFunctionKind::Recursive => {
                let max = size_arg(cx, values.get(1), "doc/chunk-recursive requires a max size")?;
                chunk(&doc, ChunkOp::Recursive { max })
            }
            DocChunkFunctionKind::Heading => chunk(&doc, ChunkOp::Heading),
        };
        if values.len() > self.kind.arity() {
            return Err(Error::Eval(format!(
                "{} expects {} argument(s)",
                self.kind.symbol(),
                self.kind.arity()
            )));
        }
        cx.factory().expr(Expr::List(
            chunks.into_iter().map(|chunk| chunk.as_expr()).collect(),
        ))
    }
}

impl Object for DocChunkFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.kind.symbol()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ObjectCompat for DocChunkFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Function"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_FUNCTION_CLASS_ID,
            Symbol::qualified("core", "Function"),
        )
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.kind.symbol()))
    }
}

impl Callable for DocCatalogFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        if !args.values().is_empty() {
            return Err(Error::Eval(format!(
                "{} expects no arguments",
                self.kind.symbol()
            )));
        }
        let expr = match self.kind {
            DocCatalogFunctionKind::BackendCatalog => backend_catalog_expr(),
            DocCatalogFunctionKind::MarkdownToTypst => markdown_to_typst_expr()?,
        };
        cx.factory().expr(expr)
    }
}

impl Object for DocCatalogFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.kind.symbol()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ObjectCompat for DocCatalogFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Function"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_FUNCTION_CLASS_ID,
            Symbol::qualified("core", "Function"),
        )
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.kind.symbol()))
    }
}

impl DocChunkFunctionKind {
    pub fn symbol(self) -> Symbol {
        match self {
            Self::Fixed => Symbol::qualified("doc", "chunk-fixed"),
            Self::Recursive => Symbol::qualified("doc", "chunk-recursive"),
            Self::Heading => Symbol::qualified("doc", "chunk-heading"),
        }
    }

    fn arity(self) -> usize {
        match self {
            Self::Fixed | Self::Recursive => 2,
            Self::Heading => 1,
        }
    }
}

impl DocCatalogFunctionKind {
    pub fn symbol(self) -> Symbol {
        match self {
            Self::BackendCatalog => Symbol::qualified("doc", "backend-catalog"),
            Self::MarkdownToTypst => Symbol::qualified("doc", "markdown-to-typst"),
        }
    }
}

fn doc_arg(cx: &mut Cx, value: Option<&Value>, kind: DocChunkFunctionKind) -> Result<DocValue> {
    let Some(value) = value else {
        return Err(Error::Eval(format!(
            "{} requires a document",
            kind.symbol()
        )));
    };
    DocValue::from_expr(&value.object().as_expr(cx)?)
}

fn backend_catalog_expr() -> Expr {
    Expr::Map(vec![
        field(
            "kind",
            Expr::Symbol(Symbol::qualified("doc", "backend-catalog")),
        ),
        field(
            "backends",
            Expr::List(
                backend_catalog()
                    .into_iter()
                    .map(|backend| {
                        Expr::Map(vec![
                            field("id", Expr::String(backend.id.as_str().to_owned())),
                            field(
                                "status",
                                Expr::String(status_name(backend.status).to_owned()),
                            ),
                            field("can-read", Expr::Bool(backend.can_read)),
                            field("can-write", Expr::Bool(backend.can_write)),
                            field("notes", Expr::String(backend.notes.to_owned())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn markdown_to_typst_expr() -> Result<Expr> {
    let source = "# Guide\n\nA **small** codec demo.\n";
    let markdown = MarkdownBackend;
    let typst = TypstBackend;
    let codec = sim_kernel::CodecId(0);
    let (doc, decode_fidelity) = markdown
        .decode(source, &MarkupDecodeOptions::default())
        .map_err(|err| err.into_kernel_error(codec))?;
    let (typst_text, encode_fidelity) = typst
        .encode(&doc, &MarkupEncodeOptions::default())
        .map_err(|err| err.into_kernel_error(codec))?;
    Ok(Expr::Map(vec![
        field("kind", Expr::Symbol(Symbol::qualified("doc", "transcode"))),
        field("from", Expr::String("markdown".to_owned())),
        field("to", Expr::String("typst".to_owned())),
        field("source", Expr::String(source.to_owned())),
        field("output", Expr::String(typst_text)),
        field(
            "decode-losses",
            Expr::String(decode_fidelity.dropped.len().to_string()),
        ),
        field(
            "encode-losses",
            Expr::String(encode_fidelity.dropped.len().to_string()),
        ),
        field("roundtrip", Expr::Bool(encode_fidelity.dropped.is_empty())),
    ]))
}

fn status_name(status: BackendStatus) -> &'static str {
    match status {
        BackendStatus::Implemented => "implemented",
        BackendStatus::Tracked => "tracked",
        BackendStatus::ExternalSiteCandidate => "external-site-candidate",
    }
}

fn field(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}

fn size_arg(cx: &mut Cx, value: Option<&Value>, message: &'static str) -> Result<usize> {
    let Some(value) = value else {
        return Err(Error::Eval(message.to_owned()));
    };
    let expr = value.object().as_expr(cx)?;
    let size = usize_from_expr(&expr)?;
    if size == 0 {
        return Err(Error::Eval(
            "chunk size must be greater than zero".to_owned(),
        ));
    }
    Ok(size)
}

fn usize_from_expr(expr: &Expr) -> Result<usize> {
    match expr {
        Expr::Number(NumberLiteral { canonical, .. }) => canonical
            .parse()
            .map_err(|_| Error::Eval("expected non-negative integer size".to_owned())),
        Expr::String(text) => text
            .parse()
            .map_err(|_| Error::Eval("expected non-negative integer size".to_owned())),
        _ => Err(Error::Eval("expected numeric size".to_owned())),
    }
}
