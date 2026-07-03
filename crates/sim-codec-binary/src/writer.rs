//! Binary frame writer (encode side).
//!
//! Defines the `BinaryWriter` that emits the frame header and side tables, then
//! walks the `Expr` (or origin-bearing `LocatedExprTree`) graph into the
//! tag-prefixed binary body.

use std::collections::BTreeMap;

use sim_kernel::{Error, Expr, LocatedExprTree, Origin, Result, Symbol, Trivia};

use crate::tables::{quote_mode_byte, usize_to_u64};
use crate::{BinaryTag, FLAG_NONE, FrameTables};

pub(crate) struct BinaryWriter {
    pub(crate) bytes: Vec<u8>,
    pub(crate) flags: u64,
    tables: FrameTables,
    lib_index: BTreeMap<String, u64>,
    symbol_index: BTreeMap<Symbol, u64>,
    domain_index: BTreeMap<Symbol, u64>,
}

impl BinaryWriter {
    pub(crate) fn new(tables: FrameTables) -> Result<Self> {
        let lib_index = tables
            .libs
            .iter()
            .enumerate()
            .map(|(index, lib)| Ok((lib.clone(), usize_to_u64(index)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let symbol_index = tables
            .symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| Ok((symbol.clone(), usize_to_u64(index)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let domain_index = tables
            .number_domains
            .iter()
            .enumerate()
            .map(|(index, symbol)| Ok((symbol.clone(), usize_to_u64(index)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Self {
            bytes: Vec::new(),
            flags: FLAG_NONE,
            tables,
            lib_index,
            symbol_index,
            domain_index,
        })
    }

    pub(crate) fn write_header(&mut self) -> Result<()> {
        self.bytes.extend_from_slice(crate::MAGIC);
        self.varuint(crate::VERSION);
        self.varuint(self.flags);
        self.varuint(usize_to_u64(self.tables.libs.len())?);
        for lib in self.tables.libs.clone() {
            self.string(&lib)?;
        }
        self.varuint(usize_to_u64(self.tables.symbols.len())?);
        for symbol in self.tables.symbols.clone() {
            self.symbol_record(&symbol)?;
        }
        self.varuint(usize_to_u64(self.tables.number_domains.len())?);
        for domain in self.tables.number_domains.clone() {
            self.symbol_record(&domain)?;
        }
        Ok(())
    }

    pub(crate) fn write_expr(&mut self, expr: &Expr) -> Result<()> {
        match expr {
            Expr::Nil => self.tag(BinaryTag::Nil),
            Expr::Bool(false) => self.tag(BinaryTag::False),
            Expr::Bool(true) => self.tag(BinaryTag::True),
            Expr::Number(number) => {
                self.tag(BinaryTag::Number)?;
                self.varuint(self.domain_id(&number.domain)?);
                self.string(&number.canonical)
            }
            Expr::Symbol(symbol) => {
                self.tag(BinaryTag::Symbol)?;
                self.varuint(self.symbol_id(symbol)?);
                Ok(())
            }
            Expr::Local(symbol) => {
                self.tag(BinaryTag::Local)?;
                self.varuint(self.symbol_id(symbol)?);
                Ok(())
            }
            Expr::String(value) => {
                self.tag(BinaryTag::String)?;
                self.string(value)
            }
            Expr::Bytes(value) => {
                self.tag(BinaryTag::Bytes)?;
                self.blob(value)
            }
            Expr::List(items) => self.expr_list(BinaryTag::List, items),
            Expr::Vector(items) => self.expr_list(BinaryTag::Vector, items),
            Expr::Map(entries) => {
                self.tag(BinaryTag::Map)?;
                let mut sorted = entries.clone();
                sorted.sort_by_key(|(key, value)| (key.canonical_key(), value.canonical_key()));
                self.varuint(usize_to_u64(sorted.len())?);
                for (key, value) in sorted {
                    self.write_expr(&key)?;
                    self.write_expr(&value)?;
                }
                Ok(())
            }
            Expr::Set(items) => {
                self.tag(BinaryTag::Set)?;
                let mut sorted = items.clone();
                sorted.sort_by_key(Expr::canonical_key);
                self.varuint(usize_to_u64(sorted.len())?);
                for item in sorted {
                    self.write_expr(&item)?;
                }
                Ok(())
            }
            Expr::Call { operator, args } => {
                self.tag(BinaryTag::Call)?;
                self.write_expr(operator)?;
                self.varuint(usize_to_u64(args.len())?);
                for arg in args {
                    self.write_expr(arg)?;
                }
                Ok(())
            }
            Expr::Infix {
                operator,
                left,
                right,
            } => {
                self.tag(BinaryTag::Infix)?;
                self.varuint(self.symbol_id(operator)?);
                self.write_expr(left)?;
                self.write_expr(right)
            }
            Expr::Prefix { operator, arg } => {
                self.tag(BinaryTag::Prefix)?;
                self.varuint(self.symbol_id(operator)?);
                self.write_expr(arg)
            }
            Expr::Postfix { operator, arg } => {
                self.tag(BinaryTag::Postfix)?;
                self.varuint(self.symbol_id(operator)?);
                self.write_expr(arg)
            }
            Expr::Block(items) => self.expr_list(BinaryTag::Block, items),
            Expr::Quote { mode, expr } => {
                self.tag(BinaryTag::Quote)?;
                self.bytes.push(quote_mode_byte(*mode));
                self.write_expr(expr)
            }
            Expr::Annotated { expr, annotations } => {
                self.tag(BinaryTag::Annotated)?;
                self.write_expr(expr)?;
                self.varuint(usize_to_u64(annotations.len())?);
                for (key, value) in annotations {
                    self.varuint(self.symbol_id(key)?);
                    self.write_expr(value)?;
                }
                Ok(())
            }
            Expr::Extension { tag, payload } => {
                self.tag(BinaryTag::Extension)?;
                self.varuint(self.symbol_id(tag)?);
                self.write_expr(payload)
            }
        }
    }

    fn expr_list(&mut self, tag: BinaryTag, items: &[Expr]) -> Result<()> {
        self.tag(tag)?;
        self.varuint(usize_to_u64(items.len())?);
        for item in items {
            self.write_expr(item)?;
        }
        Ok(())
    }

    fn tag(&mut self, tag: BinaryTag) -> Result<()> {
        self.bytes
            .try_reserve(1)
            .map_err(|err| Error::HostError(err.to_string()))?;
        self.bytes.push(tag as u8);
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<()> {
        self.varuint(usize_to_u64(value.len())?);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn blob(&mut self, value: &[u8]) -> Result<()> {
        self.varuint(usize_to_u64(value.len())?);
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn varuint(&mut self, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            self.bytes.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn symbol_record(&mut self, symbol: &Symbol) -> Result<()> {
        let namespace_id = match &symbol.namespace {
            Some(namespace) => {
                self.lib_index
                    .get(namespace.as_ref())
                    .copied()
                    .ok_or_else(|| {
                        Error::Eval(format!("missing namespace table entry {namespace}"))
                    })?
                    + 1
            }
            None => 0,
        };
        self.varuint(namespace_id);
        self.string(&symbol.name)
    }

    fn symbol_id(&self, symbol: &Symbol) -> Result<u64> {
        self.symbol_index
            .get(symbol)
            .copied()
            .ok_or_else(|| Error::Eval(format!("missing symbol table entry {symbol}")))
    }

    fn domain_id(&self, symbol: &Symbol) -> Result<u64> {
        self.domain_index
            .get(symbol)
            .copied()
            .ok_or_else(|| Error::Eval(format!("missing number domain table entry {symbol}")))
    }

    pub(crate) fn write_origin(&mut self, origin: &Origin) -> Result<()> {
        self.varuint(u64::from(origin.codec.0));
        self.string(&origin.source.0)?;
        self.varuint(usize_to_u64(origin.span.start)?);
        self.varuint(usize_to_u64(origin.span.end)?);
        self.varuint(usize_to_u64(origin.trivia.len())?);
        for trivia in &origin.trivia {
            match trivia {
                Trivia::Whitespace(text) => {
                    self.bytes.push(0);
                    self.string(text)?;
                }
                Trivia::LineComment(text) => {
                    self.bytes.push(1);
                    self.string(text)?;
                }
                Trivia::BlockComment(text) => {
                    self.bytes.push(2);
                    self.string(text)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn write_origin_tree(&mut self, tree: &LocatedExprTree) -> Result<()> {
        match &tree.origin {
            Some(origin) => {
                self.bytes.push(1);
                self.write_origin(origin)?;
            }
            None => self.bytes.push(0),
        }
        for child in &tree.children {
            self.write_origin_tree(child)?;
        }
        Ok(())
    }
}
