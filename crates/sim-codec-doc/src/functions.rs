//! The `doc/chunk-fixed`, `doc/chunk-recursive`, and `doc/chunk-heading`
//! chunking operations exposed as kernel callables, each wrapping a `ChunkOp`
//! over a decoded document.

use std::any::Any;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, NumberLiteral, Object, ObjectCompat, Result, Symbol,
    Value,
};

use crate::document::{ChunkOp, DocValue, chunk};

pub const CHUNK_FUNCTIONS: &[DocChunkFunctionKind] = &[
    DocChunkFunctionKind::Fixed,
    DocChunkFunctionKind::Recursive,
    DocChunkFunctionKind::Heading,
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

impl DocChunkFunction {
    pub fn new(kind: DocChunkFunctionKind) -> Self {
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

fn doc_arg(cx: &mut Cx, value: Option<&Value>, kind: DocChunkFunctionKind) -> Result<DocValue> {
    let Some(value) = value else {
        return Err(Error::Eval(format!(
            "{} requires a document",
            kind.symbol()
        )));
    };
    DocValue::from_expr(&value.object().as_expr(cx)?)
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
