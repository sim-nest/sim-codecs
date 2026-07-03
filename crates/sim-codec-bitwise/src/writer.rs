//! Bitwise frame writer (encode side).
//!
//! `FrameWriter` emits the self-delimiting header and side tables, then walks
//! the `Expr` (or origin-bearing `LocatedExprTree`) graph into the bit-packed,
//! tag-prefixed body. Every dynamic length and index rides `vbits`.

use std::collections::{BTreeMap, HashMap};

use sim_kernel::{Error, Expr, LocatedExprTree, NumberLiteral, Origin, Result, Symbol, Trivia};

use crate::FrameTables;
use crate::bitio::{BitWriter, write_len, write_vbits};
use crate::number::{integer_to_bits, small_uint_literal};
use crate::tables::quote_mode_bits;
use crate::types::{BitwiseTag, FLAG_NONE, VERSION};

pub(crate) struct FrameWriter {
    out: BitWriter,
    pub(crate) flags: u128,
    tables: FrameTables,
    lib_index: BTreeMap<String, usize>,
    symbol_index: BTreeMap<Symbol, usize>,
    domain_index: BTreeMap<Symbol, usize>,
    /// Dense mode: share a repeated, value-equal subtree behind a `Ref` instead
    /// of re-encoding it. Off for the plain, canonical body.
    dense: bool,
    /// Pre-order index of every subtree already fully seen in dense mode.
    seen: HashMap<Expr, usize>,
    /// Next pre-order index to assign to a freshly written (non-`Ref`) subtree.
    next_index: usize,
}

impl FrameWriter {
    pub(crate) fn new(tables: FrameTables) -> Self {
        let index_of = |items: &[String]| -> BTreeMap<String, usize> {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| (item.clone(), index))
                .collect()
        };
        let symbol_index = tables
            .symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| (symbol.clone(), index))
            .collect();
        let domain_index = tables
            .number_domains
            .iter()
            .enumerate()
            .map(|(index, symbol)| (symbol.clone(), index))
            .collect();
        Self {
            out: BitWriter::new(),
            flags: FLAG_NONE,
            lib_index: index_of(&tables.libs),
            symbol_index,
            domain_index,
            tables,
            dense: false,
            seen: HashMap::new(),
            next_index: 0,
        }
    }

    /// Enables dense structural sharing for the body written after this call.
    pub(crate) fn set_dense(&mut self, dense: bool) {
        self.dense = dense;
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.out.finish()
    }

    pub(crate) fn write_header(&mut self) -> Result<()> {
        write_vbits(&mut self.out, VERSION);
        write_vbits(&mut self.out, self.flags);
        write_len(&mut self.out, self.tables.libs.len());
        for lib in self.tables.libs.clone() {
            self.bytes_field(lib.as_bytes());
        }
        write_len(&mut self.out, self.tables.symbols.len());
        for symbol in self.tables.symbols.clone() {
            self.symbol_record(&symbol)?;
        }
        write_len(&mut self.out, self.tables.number_domains.len());
        for domain in self.tables.number_domains.clone() {
            self.symbol_record(&domain)?;
        }
        Ok(())
    }

    /// Writes one subexpression, sharing it as a `Ref` in dense mode when a
    /// value-equal subtree was already emitted.
    ///
    /// A subtree can only be value-equal to a node in an already-completed
    /// branch, never to one of its own ancestors (that would require infinite
    /// depth), so registering the pre-order index before recursing is safe: a
    /// `Ref` always points at a prior, fully written subtree.
    pub(crate) fn write_expr(&mut self, expr: &Expr) -> Result<()> {
        if self.dense {
            if let Some(&back) = self.seen.get(expr) {
                self.tag(BitwiseTag::Ref)?;
                write_vbits(&mut self.out, back as u128);
                return Ok(());
            }
            let index = self.next_index;
            self.next_index += 1;
            self.seen.insert(expr.clone(), index);
        }
        self.write_expr_body(expr)
    }

    fn write_expr_body(&mut self, expr: &Expr) -> Result<()> {
        match expr {
            Expr::Nil => self.tag(BitwiseTag::Nil),
            Expr::Bool(false) => self.tag(BitwiseTag::False),
            Expr::Bool(true) => self.tag(BitwiseTag::True),
            Expr::Number(number) => self.write_number(number),
            Expr::Symbol(symbol) => {
                self.tag(BitwiseTag::Symbol)?;
                let id = self.symbol_id(symbol)?;
                write_len(&mut self.out, id);
                Ok(())
            }
            Expr::Local(symbol) => {
                self.tag(BitwiseTag::Local)?;
                let id = self.symbol_id(symbol)?;
                write_len(&mut self.out, id);
                Ok(())
            }
            Expr::String(value) => {
                self.tag(BitwiseTag::String)?;
                self.bytes_field(value.as_bytes());
                Ok(())
            }
            Expr::Bytes(value) => {
                self.tag(BitwiseTag::Bytes)?;
                self.bytes_field(value);
                Ok(())
            }
            Expr::List(items) => self.expr_seq(BitwiseTag::List, items),
            Expr::Vector(items) => self.expr_seq(BitwiseTag::Vector, items),
            Expr::Map(entries) => {
                self.tag(BitwiseTag::Map)?;
                let mut sorted = entries.clone();
                sorted.sort_by_key(|(key, value)| (key.canonical_key(), value.canonical_key()));
                write_len(&mut self.out, sorted.len());
                for (key, value) in sorted {
                    self.write_expr(&key)?;
                    self.write_expr(&value)?;
                }
                Ok(())
            }
            Expr::Set(items) => {
                self.tag(BitwiseTag::Set)?;
                let mut sorted = items.clone();
                sorted.sort_by_key(Expr::canonical_key);
                write_len(&mut self.out, sorted.len());
                for item in sorted {
                    self.write_expr(&item)?;
                }
                Ok(())
            }
            Expr::Call { operator, args } => {
                self.tag(BitwiseTag::Call)?;
                self.write_expr(operator)?;
                write_len(&mut self.out, args.len());
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
                self.tag(BitwiseTag::Infix)?;
                let id = self.symbol_id(operator)?;
                write_len(&mut self.out, id);
                self.write_expr(left)?;
                self.write_expr(right)
            }
            Expr::Prefix { operator, arg } => {
                self.tag(BitwiseTag::Prefix)?;
                let id = self.symbol_id(operator)?;
                write_len(&mut self.out, id);
                self.write_expr(arg)
            }
            Expr::Postfix { operator, arg } => {
                self.tag(BitwiseTag::Postfix)?;
                let id = self.symbol_id(operator)?;
                write_len(&mut self.out, id);
                self.write_expr(arg)
            }
            Expr::Block(items) => self.expr_seq(BitwiseTag::Block, items),
            Expr::Quote { mode, expr } => {
                self.tag(BitwiseTag::Quote)?;
                self.out.write_bits(quote_mode_bits(*mode), 3);
                self.write_expr(expr)
            }
            Expr::Annotated { expr, annotations } => {
                self.tag(BitwiseTag::Annotated)?;
                self.write_expr(expr)?;
                write_len(&mut self.out, annotations.len());
                for (key, value) in annotations {
                    let id = self.symbol_id(key)?;
                    write_len(&mut self.out, id);
                    self.write_expr(value)?;
                }
                Ok(())
            }
            Expr::Extension { tag, payload } => {
                self.tag(BitwiseTag::Extension)?;
                let id = self.symbol_id(tag)?;
                write_len(&mut self.out, id);
                self.write_expr(payload)
            }
        }
    }

    fn write_number(&mut self, number: &NumberLiteral) -> Result<()> {
        if let Some(k) = small_uint_literal(&number.canonical) {
            let tag = BitwiseTag::from_u6(k).expect("small uint tag in range");
            self.tag(tag)?;
            let domain = self.domain_id(&number.domain)?;
            write_len(&mut self.out, domain);
            return Ok(());
        }
        self.tag(BitwiseTag::Number)?;
        let domain = self.domain_id(&number.domain)?;
        write_len(&mut self.out, domain);
        match integer_to_bits(&number.canonical) {
            Some((negative, bits)) => {
                self.out.write_bit(true); // mode = Integer
                self.out.write_bit(negative); // sign
                write_len(&mut self.out, bits.len());
                for bit in bits {
                    self.out.write_bit(bit);
                }
            }
            None => {
                self.out.write_bit(false); // mode = Text
                self.bytes_field(number.canonical.as_bytes());
            }
        }
        Ok(())
    }

    fn expr_seq(&mut self, tag: BitwiseTag, items: &[Expr]) -> Result<()> {
        self.tag(tag)?;
        write_len(&mut self.out, items.len());
        for item in items {
            self.write_expr(item)?;
        }
        Ok(())
    }

    fn tag(&mut self, tag: BitwiseTag) -> Result<()> {
        self.out.write_bits(tag as u128, BitwiseTag::WIDTH_BITS);
        Ok(())
    }

    fn bytes_field(&mut self, bytes: &[u8]) {
        write_len(&mut self.out, bytes.len());
        self.out.write_bytes(bytes);
    }

    fn symbol_record(&mut self, symbol: &Symbol) -> Result<()> {
        let namespace_id = match &symbol.namespace {
            Some(namespace) => {
                let index = self
                    .lib_index
                    .get(namespace.as_ref())
                    .copied()
                    .ok_or_else(|| {
                        Error::Eval(format!("missing namespace table entry {namespace}"))
                    })?;
                index + 1
            }
            None => 0,
        };
        write_len(&mut self.out, namespace_id);
        self.bytes_field(symbol.name.as_bytes());
        Ok(())
    }

    fn symbol_id(&self, symbol: &Symbol) -> Result<usize> {
        self.symbol_index
            .get(symbol)
            .copied()
            .ok_or_else(|| Error::Eval(format!("missing symbol table entry {symbol}")))
    }

    fn domain_id(&self, symbol: &Symbol) -> Result<usize> {
        self.domain_index
            .get(symbol)
            .copied()
            .ok_or_else(|| Error::Eval(format!("missing number domain table entry {symbol}")))
    }

    pub(crate) fn write_origin(&mut self, origin: &Origin) -> Result<()> {
        write_vbits(&mut self.out, u128::from(origin.codec.0));
        self.bytes_field(origin.source.0.as_bytes());
        write_len(&mut self.out, origin.span.start);
        write_len(&mut self.out, origin.span.end);
        write_len(&mut self.out, origin.trivia.len());
        for trivia in &origin.trivia {
            let (kind, text) = match trivia {
                Trivia::Whitespace(text) => (0u128, text),
                Trivia::LineComment(text) => (1u128, text),
                Trivia::BlockComment(text) => (2u128, text),
            };
            self.out.write_bits(kind, 2);
            self.bytes_field(text.as_bytes());
        }
        Ok(())
    }

    pub(crate) fn write_origin_tree(&mut self, tree: &LocatedExprTree) -> Result<()> {
        match &tree.origin {
            Some(origin) => {
                self.out.write_bit(true);
                self.write_origin(origin)?;
            }
            None => self.out.write_bit(false),
        }
        for child in &tree.children {
            self.write_origin_tree(child)?;
        }
        Ok(())
    }
}
