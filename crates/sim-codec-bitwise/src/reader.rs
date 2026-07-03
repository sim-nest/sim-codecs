//! Bitwise frame reader (decode side).
//!
//! `FrameReader` parses the self-delimiting header and side tables, then the
//! bit-packed `Expr` body and optional origin payloads, all under
//! [`crate::DecodeLimits`]. Every count and length is checked before any
//! allocation, so a malformed or hostile frame fails closed.

use sim_kernel::{
    Error, Expr, LocatedExprTree, NumberLiteral, Origin, QuoteMode, Result, SourceId, Span, Symbol,
    Trivia,
};

use crate::bitio::{BitReader, read_len, read_vbits};
use crate::number::bits_to_integer;
use crate::tables::quote_mode_from_bits;
use crate::types::{BitwiseTag, FLAG_DENSE, FLAG_KNOWN};
use crate::{DecodeLimits, FrameTables};

/// Cap on the initial capacity reserved for an attacker-declared length. The
/// declared length is bounds-checked, but eager `Vec::with_capacity(len)` at
/// every nesting level lets a small, deeply nested input force huge
/// reservations; we reserve at most this many slots and let vectors grow.
const ALLOC_RESERVE_CAP: usize = 4096;

pub(crate) struct FrameReader<'a> {
    input: BitReader<'a>,
    codec: sim_kernel::CodecId,
    pub(crate) flags: u128,
    tables: Option<FrameTables>,
    limits: DecodeLimits,
    expr_nodes: usize,
    /// Dense mode: resolve `Ref` back-references while decoding the body.
    dense: bool,
    /// Completed subtrees by pre-order index, each with its node count. A slot
    /// is `None` while its subtree is still being decoded, so a `Ref` to an
    /// unfinished (ancestor or forward) node fails closed.
    dense_nodes: Vec<Option<(Expr, usize)>>,
}

impl<'a> FrameReader<'a> {
    pub(crate) fn new(
        codec: sim_kernel::CodecId,
        bytes: &'a [u8],
        limits: DecodeLimits,
    ) -> Result<Self> {
        let input = BitReader::new(codec, bytes, limits)?;
        Ok(Self {
            input,
            codec,
            flags: 0,
            tables: None,
            limits,
            expr_nodes: 0,
            dense: false,
            dense_nodes: Vec::new(),
        })
    }

    pub(crate) fn require_zero_padding(&mut self) -> Result<()> {
        self.input.require_zero_padding()
    }

    pub(crate) fn read_header(&mut self) -> Result<FrameTables> {
        let version = read_vbits(&mut self.input)?;
        if version != crate::types::VERSION {
            return Err(self.error(format!("unsupported bitwise frame version {version}")));
        }
        let flags = read_vbits(&mut self.input)?;
        if flags & !FLAG_KNOWN != 0 {
            return Err(self.error(format!("unsupported bitwise frame flags {flags}")));
        }
        self.flags = flags;
        self.dense = flags & FLAG_DENSE != 0;

        let libs_len = self.read_count("lib table")?;
        let mut libs = Vec::with_capacity(libs_len.min(ALLOC_RESERVE_CAP));
        for _ in 0..libs_len {
            libs.push(self.read_string()?);
        }

        let symbols_len = self.read_count("symbol table")?;
        let mut symbols = Vec::with_capacity(symbols_len.min(ALLOC_RESERVE_CAP));
        for _ in 0..symbols_len {
            symbols.push(self.read_symbol_record(&libs)?);
        }

        let domains_len = self.read_count("number domain table")?;
        let mut number_domains = Vec::with_capacity(domains_len.min(ALLOC_RESERVE_CAP));
        for _ in 0..domains_len {
            number_domains.push(self.read_symbol_record(&libs)?);
        }

        let tables = FrameTables {
            libs,
            symbols,
            number_domains,
        };
        self.tables = Some(tables.clone());
        Ok(tables)
    }

    pub(crate) fn read_expr(&mut self) -> Result<Expr> {
        self.read_expr_with_depth(0)
    }

    fn read_expr_with_depth(&mut self, depth: usize) -> Result<Expr> {
        self.check_depth(depth)?;
        let tag = self.read_tag()?;
        if self.dense && tag == BitwiseTag::Ref {
            return self.resolve_ref();
        }
        // Reserve this node's pre-order slot before decoding its subtree so a
        // later `Ref` to it can be resolved (and an in-progress slot stays
        // `None`, rejecting ancestor/forward references).
        let slot = if self.dense {
            let slot = self.dense_nodes.len();
            self.dense_nodes.push(None);
            Some(slot)
        } else {
            None
        };
        self.bump_expr_nodes()?;
        let expr = self.read_tagged(tag, depth)?;
        if let Some(slot) = slot {
            let count = expr_node_count(&expr);
            self.dense_nodes[slot] = Some((expr.clone(), count));
        }
        Ok(expr)
    }

    /// Resolves a dense `Ref` to a prior, fully decoded subtree, charging its
    /// expanded node count against the decode limit to bound decompression.
    fn resolve_ref(&mut self) -> Result<Expr> {
        let raw = read_vbits(&mut self.input)?;
        let index =
            usize::try_from(raw).map_err(|_| self.error("dense ref index overflows usize"))?;
        let entry = self
            .dense_nodes
            .get(index)
            .ok_or_else(|| self.error(format!("dense ref index {index} out of range")))?;
        let (expr, count) = entry
            .clone()
            .ok_or_else(|| self.error(format!("dense ref index {index} is a forward reference")))?;
        self.bump_expr_nodes_by(count)?;
        Ok(expr)
    }

    fn read_tagged(&mut self, tag: BitwiseTag, depth: usize) -> Result<Expr> {
        if let Some(k) = tag.small_uint() {
            return Ok(Expr::Number(self.read_small_uint(k)?));
        }
        match tag {
            BitwiseTag::Nil => Ok(Expr::Nil),
            BitwiseTag::False => Ok(Expr::Bool(false)),
            BitwiseTag::True => Ok(Expr::Bool(true)),
            BitwiseTag::Number => Ok(Expr::Number(self.read_number()?)),
            BitwiseTag::Symbol => Ok(Expr::Symbol(self.read_symbol()?)),
            BitwiseTag::Local => Ok(Expr::Local(self.read_symbol()?)),
            BitwiseTag::String => Ok(Expr::String(self.read_string()?)),
            BitwiseTag::Bytes => Ok(Expr::Bytes(self.read_blob()?)),
            BitwiseTag::List => Ok(Expr::List(self.read_expr_vec(depth + 1)?)),
            BitwiseTag::Vector => Ok(Expr::Vector(self.read_expr_vec(depth + 1)?)),
            BitwiseTag::Map => {
                let len = self.read_count("map entries")?;
                let mut entries = Vec::with_capacity(len.min(ALLOC_RESERVE_CAP));
                for _ in 0..len {
                    let key = self.read_expr_with_depth(depth + 1)?;
                    let value = self.read_expr_with_depth(depth + 1)?;
                    entries.push((key, value));
                }
                Ok(Expr::Map(entries))
            }
            BitwiseTag::Set => Ok(Expr::Set(self.read_expr_vec(depth + 1)?)),
            BitwiseTag::Call => {
                let operator = Box::new(self.read_expr_with_depth(depth + 1)?);
                let args = self.read_expr_vec(depth + 1)?;
                Ok(Expr::Call { operator, args })
            }
            BitwiseTag::Infix => Ok(Expr::Infix {
                operator: self.read_symbol()?,
                left: Box::new(self.read_expr_with_depth(depth + 1)?),
                right: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BitwiseTag::Prefix => Ok(Expr::Prefix {
                operator: self.read_symbol()?,
                arg: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BitwiseTag::Postfix => Ok(Expr::Postfix {
                operator: self.read_symbol()?,
                arg: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BitwiseTag::Block => Ok(Expr::Block(self.read_expr_vec(depth + 1)?)),
            BitwiseTag::Quote => Ok(Expr::Quote {
                mode: self.read_quote_mode()?,
                expr: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BitwiseTag::Annotated => {
                let expr = Box::new(self.read_expr_with_depth(depth + 1)?);
                let len = self.read_count("annotation entries")?;
                let mut annotations = Vec::with_capacity(len.min(ALLOC_RESERVE_CAP));
                for _ in 0..len {
                    let key = self.read_symbol()?;
                    let value = self.read_expr_with_depth(depth + 1)?;
                    annotations.push((key, value));
                }
                Ok(Expr::Annotated { expr, annotations })
            }
            BitwiseTag::Extension => Ok(Expr::Extension {
                tag: self.read_symbol()?,
                payload: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BitwiseTag::Ref => Err(self.error("dense-mode Ref tag is not supported")),
            BitwiseTag::UInt0
            | BitwiseTag::UInt1
            | BitwiseTag::UInt2
            | BitwiseTag::UInt3
            | BitwiseTag::UInt4
            | BitwiseTag::UInt5
            | BitwiseTag::UInt6
            | BitwiseTag::UInt7
            | BitwiseTag::UInt8
            | BitwiseTag::UInt9
            | BitwiseTag::UInt10
            | BitwiseTag::UInt11
            | BitwiseTag::UInt12
            | BitwiseTag::UInt13
            | BitwiseTag::UInt14
            | BitwiseTag::UInt15 => {
                // Handled above by `tag.small_uint()`.
                Err(self.error("inline uint tag reached structural decode"))
            }
        }
    }

    fn read_expr_vec(&mut self, depth: usize) -> Result<Vec<Expr>> {
        let len = self.read_count("expr list")?;
        let mut items = Vec::with_capacity(len.min(ALLOC_RESERVE_CAP));
        for _ in 0..len {
            items.push(self.read_expr_with_depth(depth)?);
        }
        Ok(items)
    }

    fn read_small_uint(&mut self, value: u8) -> Result<NumberLiteral> {
        let domain = self.read_number_domain()?;
        Ok(NumberLiteral {
            domain,
            canonical: value.to_string(),
        })
    }

    fn read_number(&mut self) -> Result<NumberLiteral> {
        let domain = self.read_number_domain()?;
        let integer_mode = self.input.read_bit()?;
        let canonical = if integer_mode {
            let negative = self.input.read_bit()?;
            let bit_count = read_len(
                &mut self.input,
                self.limits.max_string_bytes.saturating_mul(8),
            )?;
            let mut bits = Vec::with_capacity(bit_count.min(ALLOC_RESERVE_CAP));
            for _ in 0..bit_count {
                bits.push(self.input.read_bit()?);
            }
            if let Some(&false) = bits.first() {
                return Err(self.error("integer magnitude carries a leading zero bit"));
            }
            bits_to_integer(negative, &bits)
        } else {
            self.read_string()?
        };
        Ok(NumberLiteral { domain, canonical })
    }

    fn read_tag(&mut self) -> Result<BitwiseTag> {
        let value = self.input.read_bits(BitwiseTag::WIDTH_BITS)? as u8;
        BitwiseTag::from_u6(value)
            .ok_or_else(|| self.error(format!("reserved bitwise tag {value}")))
    }

    fn read_symbol(&mut self) -> Result<Symbol> {
        let index = self.read_index()?;
        self.tables()?
            .symbols
            .get(index)
            .cloned()
            .ok_or_else(|| self.error(format!("symbol index {index} out of range")))
    }

    fn read_number_domain(&mut self) -> Result<Symbol> {
        let index = self.read_index()?;
        self.tables()?
            .number_domains
            .get(index)
            .cloned()
            .ok_or_else(|| self.error(format!("number domain index {index} out of range")))
    }

    fn read_symbol_record(&mut self, libs: &[String]) -> Result<Symbol> {
        let namespace_slot = read_vbits(&mut self.input)?;
        let name = self.read_string()?;
        if namespace_slot == 0 {
            return Ok(Symbol::new(name));
        }
        let index = usize::try_from(namespace_slot - 1)
            .map_err(|_| self.error("namespace index overflows usize"))?;
        let namespace = libs
            .get(index)
            .cloned()
            .ok_or_else(|| self.error(format!("namespace index {index} out of range")))?;
        Ok(Symbol::qualified(namespace, name))
    }

    pub(crate) fn read_origin(&mut self) -> Result<Origin> {
        let codec = read_vbits(&mut self.input)?;
        let codec =
            u32::try_from(codec).map_err(|_| self.error("origin codec id overflows u32"))?;
        let source = self.read_string()?;
        let start = self.read_index()?;
        let end = self.read_index()?;
        let trivia_len =
            self.read_count_with_limit("origin trivia", self.limits.max_trivia_items)?;
        let mut trivia = Vec::with_capacity(trivia_len.min(ALLOC_RESERVE_CAP));
        for _ in 0..trivia_len {
            let kind = self.input.read_bits(2)?;
            let text = self.read_string()?;
            let item = match kind {
                0 => Trivia::Whitespace(text),
                1 => Trivia::LineComment(text),
                2 => Trivia::BlockComment(text),
                other => return Err(self.error(format!("unknown trivia kind {other}"))),
            };
            trivia.push(item);
        }
        Ok(Origin {
            codec: sim_kernel::CodecId(codec),
            source: SourceId(source),
            span: Span { start, end },
            trivia,
        })
    }

    pub(crate) fn read_origin_tree(&mut self, expr: Expr) -> Result<LocatedExprTree> {
        self.read_origin_tree_with_depth(expr, 0)
    }

    fn read_origin_tree_with_depth(&mut self, expr: Expr, depth: usize) -> Result<LocatedExprTree> {
        self.check_depth(depth)?;
        let origin = if self.input.read_bit()? {
            Some(self.read_origin()?)
        } else {
            None
        };
        match expr {
            Expr::Nil
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Symbol(_)
            | Expr::Local(_)
            | Expr::String(_)
            | Expr::Bytes(_) => Ok(LocatedExprTree::without_children(expr, origin)),
            Expr::List(items) => self.seq_tree(items, origin, Expr::List, depth + 1),
            Expr::Vector(items) => self.seq_tree(items, origin, Expr::Vector, depth + 1),
            Expr::Set(items) => self.seq_tree(items, origin, Expr::Set, depth + 1),
            Expr::Block(items) => self.seq_tree(items, origin, Expr::Block, depth + 1),
            Expr::Map(entries) => {
                let mut expr_entries = Vec::with_capacity(entries.len());
                let mut children = Vec::with_capacity(entries.len() * 2);
                for (key, value) in entries {
                    let key_tree = self.read_origin_tree_with_depth(key, depth + 1)?;
                    let value_tree = self.read_origin_tree_with_depth(value, depth + 1)?;
                    expr_entries.push((key_tree.expr.clone(), value_tree.expr.clone()));
                    children.push(key_tree);
                    children.push(value_tree);
                }
                Ok(LocatedExprTree {
                    expr: Expr::Map(expr_entries),
                    origin,
                    children,
                })
            }
            Expr::Call { operator, args } => {
                let operator_tree = self.read_origin_tree_with_depth(*operator, depth + 1)?;
                let arg_trees = args
                    .into_iter()
                    .map(|arg| self.read_origin_tree_with_depth(arg, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                let mut children = Vec::with_capacity(arg_trees.len() + 1);
                children.push(operator_tree.clone());
                children.extend(arg_trees.iter().cloned());
                Ok(LocatedExprTree {
                    expr: Expr::Call {
                        operator: Box::new(operator_tree.expr.clone()),
                        args: arg_trees.iter().map(|arg| arg.expr.clone()).collect(),
                    },
                    origin,
                    children,
                })
            }
            Expr::Infix {
                operator,
                left,
                right,
            } => {
                let left_tree = self.read_origin_tree_with_depth(*left, depth + 1)?;
                let right_tree = self.read_origin_tree_with_depth(*right, depth + 1)?;
                Ok(LocatedExprTree {
                    expr: Expr::Infix {
                        operator,
                        left: Box::new(left_tree.expr.clone()),
                        right: Box::new(right_tree.expr.clone()),
                    },
                    origin,
                    children: vec![left_tree, right_tree],
                })
            }
            Expr::Prefix { operator, arg } => {
                let arg_tree = self.read_origin_tree_with_depth(*arg, depth + 1)?;
                Ok(LocatedExprTree {
                    expr: Expr::Prefix {
                        operator,
                        arg: Box::new(arg_tree.expr.clone()),
                    },
                    origin,
                    children: vec![arg_tree],
                })
            }
            Expr::Postfix { operator, arg } => {
                let arg_tree = self.read_origin_tree_with_depth(*arg, depth + 1)?;
                Ok(LocatedExprTree {
                    expr: Expr::Postfix {
                        operator,
                        arg: Box::new(arg_tree.expr.clone()),
                    },
                    origin,
                    children: vec![arg_tree],
                })
            }
            Expr::Quote { mode, expr } => {
                let expr_tree = self.read_origin_tree_with_depth(*expr, depth + 1)?;
                Ok(LocatedExprTree {
                    expr: Expr::Quote {
                        mode,
                        expr: Box::new(expr_tree.expr.clone()),
                    },
                    origin,
                    children: vec![expr_tree],
                })
            }
            Expr::Annotated { expr, annotations } => {
                let expr_tree = self.read_origin_tree_with_depth(*expr, depth + 1)?;
                let mut annotation_trees = Vec::with_capacity(annotations.len());
                for (key, value) in annotations {
                    annotation_trees
                        .push((key, self.read_origin_tree_with_depth(value, depth + 1)?));
                }
                let mut children = Vec::with_capacity(annotation_trees.len() + 1);
                children.push(expr_tree.clone());
                children.extend(annotation_trees.iter().map(|(_, value)| value.clone()));
                Ok(LocatedExprTree {
                    expr: Expr::Annotated {
                        expr: Box::new(expr_tree.expr.clone()),
                        annotations: annotation_trees
                            .iter()
                            .map(|(key, value)| (key.clone(), value.expr.clone()))
                            .collect(),
                    },
                    origin,
                    children,
                })
            }
            Expr::Extension { tag, payload } => {
                let payload_tree = self.read_origin_tree_with_depth(*payload, depth + 1)?;
                Ok(LocatedExprTree {
                    expr: Expr::Extension {
                        tag,
                        payload: Box::new(payload_tree.expr.clone()),
                    },
                    origin,
                    children: vec![payload_tree],
                })
            }
        }
    }

    fn seq_tree(
        &mut self,
        items: Vec<Expr>,
        origin: Option<Origin>,
        build: fn(Vec<Expr>) -> Expr,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        let children = items
            .into_iter()
            .map(|item| self.read_origin_tree_with_depth(item, depth))
            .collect::<Result<Vec<_>>>()?;
        Ok(LocatedExprTree {
            expr: build(children.iter().map(|item| item.expr.clone()).collect()),
            origin,
            children,
        })
    }

    fn read_quote_mode(&mut self) -> Result<QuoteMode> {
        let bits = self.input.read_bits(3)?;
        quote_mode_from_bits(bits).ok_or_else(|| self.error(format!("unknown quote mode {bits}")))
    }

    fn read_string(&mut self) -> Result<String> {
        let bytes = self.read_bytes_field(self.limits.max_string_bytes, "string")?;
        String::from_utf8(bytes).map_err(|err| self.error(err.to_string()))
    }

    fn read_blob(&mut self) -> Result<Vec<u8>> {
        self.read_bytes_field(self.limits.max_blob_bytes, "byte blob")
    }

    fn read_bytes_field(&mut self, limit: usize, kind: &str) -> Result<Vec<u8>> {
        let len = self.read_count_with_limit(kind, limit)?;
        self.input.read_bytes(len)
    }

    fn read_index(&mut self) -> Result<usize> {
        let value = read_vbits(&mut self.input)?;
        usize::try_from(value).map_err(|_| self.error("index overflows usize"))
    }

    fn read_count(&mut self, kind: &str) -> Result<usize> {
        self.read_count_with_limit(kind, self.limits.max_table_entries)
    }

    fn read_count_with_limit(&mut self, kind: &str, limit: usize) -> Result<usize> {
        let value = read_vbits(&mut self.input)?;
        let len = usize::try_from(value)
            .map_err(|_| self.error(format!("{kind} length overflows usize")))?;
        if len > limit {
            return Err(self.error(format!("{kind} exceeds decode limit: {len} > {limit}")));
        }
        Ok(len)
    }

    fn tables(&self) -> Result<&FrameTables> {
        self.tables
            .as_ref()
            .ok_or_else(|| self.error("bitwise frame header has not been read"))
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error::CodecError {
            codec: self.codec,
            message: message.into(),
        }
    }

    fn bump_expr_nodes(&mut self) -> Result<()> {
        self.bump_expr_nodes_by(1)
    }

    fn bump_expr_nodes_by(&mut self, count: usize) -> Result<()> {
        self.expr_nodes = self
            .expr_nodes
            .checked_add(count)
            .ok_or_else(|| self.error("expr node count overflow"))?;
        if self.expr_nodes > self.limits.max_expr_nodes {
            return Err(self.error(format!(
                "expr node count exceeds decode limit: {} > {}",
                self.expr_nodes, self.limits.max_expr_nodes
            )));
        }
        Ok(())
    }

    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth > self.limits.max_depth {
            return Err(self.error(format!(
                "decode nesting depth exceeds limit: {depth} > {}",
                self.limits.max_depth
            )));
        }
        Ok(())
    }
}

/// Counts the nodes in an `Expr` tree, matching the writer's pre-order walk so a
/// dense `Ref` can charge the full expanded cost against the decode limit.
fn expr_node_count(expr: &Expr) -> usize {
    1 + match expr {
        Expr::Nil
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Symbol(_)
        | Expr::Local(_)
        | Expr::String(_)
        | Expr::Bytes(_) => 0,
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.iter().map(expr_node_count).sum()
        }
        Expr::Map(entries) => entries
            .iter()
            .map(|(key, value)| expr_node_count(key) + expr_node_count(value))
            .sum(),
        Expr::Call { operator, args } => {
            expr_node_count(operator) + args.iter().map(expr_node_count).sum::<usize>()
        }
        Expr::Infix { left, right, .. } => expr_node_count(left) + expr_node_count(right),
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => expr_node_count(arg),
        Expr::Quote { expr, .. } => expr_node_count(expr),
        Expr::Annotated { expr, annotations } => {
            expr_node_count(expr)
                + annotations
                    .iter()
                    .map(|(_, value)| expr_node_count(value))
                    .sum::<usize>()
        }
        Expr::Extension { payload, .. } => expr_node_count(payload),
    }
}
