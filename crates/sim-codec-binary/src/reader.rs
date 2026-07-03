//! Binary frame reader (decode side).
//!
//! Defines the `BinaryReader` cursor that parses the frame header, side
//! tables, and primitives under [`crate::DecodeLimits`], and aggregates its
//! submodules: `expr` (reads the `Expr` body) and `tree` (reads origin-bearing
//! located trees).

mod expr;
mod tree;

use sim_kernel::{Error, Origin, QuoteMode, Result, SourceId, Span, Symbol, Trivia};

use crate::tables::u64_to_usize;
use crate::{
    BinaryTag, DecodeLimits, FLAG_NONE, FLAG_ORIGIN, FLAG_TREE_ORIGIN, FrameTables, MAGIC, VERSION,
};

pub(crate) struct BinaryReader<'a> {
    codec: sim_kernel::CodecId,
    bytes: &'a [u8],
    index: usize,
    pub(crate) flags: u64,
    tables: Option<FrameTables>,
    limits: DecodeLimits,
    expr_nodes: usize,
}

impl<'a> BinaryReader<'a> {
    pub(crate) fn new(
        codec: sim_kernel::CodecId,
        bytes: &'a [u8],
        limits: DecodeLimits,
    ) -> Result<Self> {
        if bytes.len() > limits.max_frame_bytes {
            return Err(Error::CodecError {
                codec,
                message: format!(
                    "binary frame exceeds decode limit: {} > {} bytes",
                    bytes.len(),
                    limits.max_frame_bytes
                ),
            });
        }
        Ok(Self {
            codec,
            bytes,
            index: 0,
            flags: FLAG_NONE,
            tables: None,
            limits,
            expr_nodes: 0,
        })
    }

    pub(crate) fn read_header(&mut self) -> Result<FrameTables> {
        if self.take_exact(4)? != MAGIC {
            return Err(self.error("binary frame magic mismatch"));
        }
        let version = self.read_varuint()?;
        if version != VERSION {
            return Err(self.error(format!("unsupported binary frame version {version}")));
        }
        let flags = self.read_varuint()?;
        if flags & !(FLAG_ORIGIN | FLAG_TREE_ORIGIN) != 0 {
            return Err(self.error(format!("unsupported binary frame flags {flags}")));
        }
        self.flags = flags;

        let libs_len = self.read_count("lib table")?;
        let mut libs = Vec::with_capacity(libs_len);
        for _ in 0..libs_len {
            libs.push(self.read_string()?);
        }

        let symbols_len = self.read_count("symbol table")?;
        let mut symbols = Vec::with_capacity(symbols_len);
        for _ in 0..symbols_len {
            symbols.push(self.read_symbol_record(&libs)?);
        }

        let domains_len = self.read_count("number domain table")?;
        let mut number_domains = Vec::with_capacity(domains_len);
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

    fn read_tag(&mut self) -> Result<BinaryTag> {
        let byte = self.read_u8()?;
        BinaryTag::from_byte(byte)
            .ok_or_else(|| self.error(format!("unknown binary tag 0x{byte:02x}")))
    }

    fn read_symbol(&mut self) -> Result<Symbol> {
        let index = self.read_len()?;
        self.tables()?
            .symbols
            .get(index)
            .cloned()
            .ok_or_else(|| self.error(format!("symbol index {index} out of range")))
    }

    fn read_number_domain(&mut self) -> Result<Symbol> {
        let index = self.read_len()?;
        self.tables()?
            .number_domains
            .get(index)
            .cloned()
            .ok_or_else(|| self.error(format!("number domain index {index} out of range")))
    }

    fn read_symbol_record(&mut self, libs: &[String]) -> Result<Symbol> {
        let namespace_slot = self.read_varuint()?;
        let name = self.read_string()?;
        if namespace_slot == 0 {
            return Ok(Symbol::new(name));
        }
        let namespace_index = u64_to_usize(namespace_slot - 1)?;
        let namespace = libs
            .get(namespace_index)
            .cloned()
            .ok_or_else(|| self.error(format!("namespace index {namespace_index} out of range")))?;
        Ok(Symbol::qualified(namespace, name))
    }

    pub(crate) fn read_origin(&mut self) -> Result<Origin> {
        let codec = self.read_varuint()?;
        let source = self.read_string()?;
        let start = self.read_len()?;
        let end = self.read_len()?;
        let trivia_len =
            self.read_count_with_limit("origin trivia", self.limits.max_trivia_items)?;
        let mut trivia = Vec::with_capacity(trivia_len);
        for _ in 0..trivia_len {
            let tag = self.read_u8()?;
            let text = self.read_string()?;
            let item = match tag {
                0 => Trivia::Whitespace(text),
                1 => Trivia::LineComment(text),
                2 => Trivia::BlockComment(text),
                other => return Err(self.error(format!("unknown trivia tag {other}"))),
            };
            trivia.push(item);
        }
        Ok(Origin {
            codec: sim_kernel::CodecId(codec as u32),
            source: SourceId(source),
            span: Span { start, end },
            trivia,
        })
    }

    fn read_quote_mode(&mut self) -> Result<QuoteMode> {
        let byte = self.read_u8()?;
        match byte {
            0 => Ok(QuoteMode::Quote),
            1 => Ok(QuoteMode::QuasiQuote),
            2 => Ok(QuoteMode::Unquote),
            3 => Ok(QuoteMode::Splice),
            4 => Ok(QuoteMode::Syntax),
            other => Err(self.error(format!("unknown quote mode tag {other}"))),
        }
    }

    fn read_string(&mut self) -> Result<String> {
        let bytes = self.read_blob_with_limit(self.limits.max_string_bytes, "string")?;
        String::from_utf8(bytes).map_err(|err| self.error(err.to_string()))
    }

    fn read_blob(&mut self) -> Result<Vec<u8>> {
        self.read_blob_with_limit(self.limits.max_blob_bytes, "byte blob")
    }

    fn read_blob_with_limit(&mut self, limit: usize, kind: &str) -> Result<Vec<u8>> {
        let len = self.read_count_with_limit(kind, limit)?;
        Ok(self.take_exact(len)?.to_vec())
    }

    fn read_len(&mut self) -> Result<usize> {
        u64_to_usize(self.read_varuint()?)
    }

    fn read_count(&mut self, kind: &str) -> Result<usize> {
        self.read_count_with_limit(kind, self.limits.max_table_entries)
    }

    fn read_count_with_limit(&mut self, kind: &str, limit: usize) -> Result<usize> {
        let len = self.read_len()?;
        if len > limit {
            return Err(self.error(format!("{kind} exceeds decode limit: {len} > {limit}")));
        }
        Ok(len)
    }

    fn read_varuint(&mut self) -> Result<u64> {
        let mut shift = 0u32;
        let mut value = 0u64;
        loop {
            let byte = self.read_u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            if (byte & 0x80) == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 64 {
                return Err(self.error("varuint is too large"));
            }
        }
    }

    fn read_u8(&mut self) -> Result<u8> {
        let byte = *self
            .bytes
            .get(self.index)
            .ok_or_else(|| self.error("unexpected end of binary frame"))?;
        self.index += 1;
        Ok(byte)
    }

    fn take_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .index
            .checked_add(len)
            .ok_or_else(|| self.error("binary frame length overflow"))?;
        let slice = self
            .bytes
            .get(self.index..end)
            .ok_or_else(|| self.error("unexpected end of binary frame"))?;
        self.index = end;
        Ok(slice)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.index >= self.bytes.len()
    }

    fn tables(&self) -> Result<&FrameTables> {
        self.tables
            .as_ref()
            .ok_or_else(|| self.error("binary frame header has not been read"))
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error::CodecError {
            codec: self.codec,
            message: message.into(),
        }
    }

    fn bump_expr_nodes(&mut self) -> Result<()> {
        self.expr_nodes = self
            .expr_nodes
            .checked_add(1)
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
