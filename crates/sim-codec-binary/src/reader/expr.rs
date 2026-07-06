//! Reads the tag-prefixed `Expr` body of a binary frame.
//!
//! Extends `BinaryReader` with the recursive, depth-bounded decode that turns
//! the tagged byte stream back into a kernel `Expr`.

use sim_kernel::{Expr, NumberLiteral, Result};

use crate::BinaryTag;

use super::{ALLOC_RESERVE_CAP, BinaryReader};

impl<'a> BinaryReader<'a> {
    pub(crate) fn read_expr(&mut self) -> Result<Expr> {
        self.read_expr_with_depth(0)
    }

    fn read_expr_with_depth(&mut self, depth: usize) -> Result<Expr> {
        self.check_depth(depth)?;
        self.bump_expr_nodes()?;
        let tag = self.read_tag()?;
        match tag {
            BinaryTag::Nil => Ok(Expr::Nil),
            BinaryTag::False => Ok(Expr::Bool(false)),
            BinaryTag::True => Ok(Expr::Bool(true)),
            BinaryTag::Number => Ok(Expr::Number(NumberLiteral {
                domain: self.read_number_domain()?,
                canonical: self.read_string()?,
            })),
            BinaryTag::Symbol => Ok(Expr::Symbol(self.read_symbol()?)),
            BinaryTag::Local => Ok(Expr::Local(self.read_symbol()?)),
            BinaryTag::String => Ok(Expr::String(self.read_string()?)),
            BinaryTag::Bytes => Ok(Expr::Bytes(self.read_blob()?)),
            BinaryTag::List => Ok(Expr::List(self.read_expr_vec(depth + 1)?)),
            BinaryTag::Vector => Ok(Expr::Vector(self.read_expr_vec(depth + 1)?)),
            BinaryTag::Map => {
                let len = self.read_count("map entries")?;
                let mut entries = Vec::with_capacity(len.min(ALLOC_RESERVE_CAP));
                for _ in 0..len {
                    let key = self.read_expr_with_depth(depth + 1)?;
                    let value = self.read_expr_with_depth(depth + 1)?;
                    entries.push((key, value));
                }
                Ok(Expr::Map(entries))
            }
            BinaryTag::Set => Ok(Expr::Set(self.read_expr_vec(depth + 1)?)),
            BinaryTag::Call => {
                let operator = Box::new(self.read_expr_with_depth(depth + 1)?);
                let args = self.read_expr_vec(depth + 1)?;
                Ok(Expr::Call { operator, args })
            }
            BinaryTag::Infix => Ok(Expr::Infix {
                operator: self.read_symbol()?,
                left: Box::new(self.read_expr_with_depth(depth + 1)?),
                right: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BinaryTag::Prefix => Ok(Expr::Prefix {
                operator: self.read_symbol()?,
                arg: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BinaryTag::Postfix => Ok(Expr::Postfix {
                operator: self.read_symbol()?,
                arg: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BinaryTag::Block => Ok(Expr::Block(self.read_expr_vec(depth + 1)?)),
            BinaryTag::Quote => Ok(Expr::Quote {
                mode: self.read_quote_mode()?,
                expr: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
            BinaryTag::Annotated => {
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
            BinaryTag::Extension => Ok(Expr::Extension {
                tag: self.read_symbol()?,
                payload: Box::new(self.read_expr_with_depth(depth + 1)?),
            }),
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
}
