//! Reads origin-bearing located trees from a binary frame.
//!
//! Extends `BinaryReader` to reattach per-node source origins, rebuilding a
//! `LocatedExprTree` from an already-decoded `Expr` and the frame's origin
//! data.

use sim_kernel::{Expr, LocatedExprTree, Origin, Result};

use super::BinaryReader;

impl<'a> BinaryReader<'a> {
    pub(crate) fn read_origin_tree(&mut self, expr: Expr) -> Result<LocatedExprTree> {
        self.read_origin_tree_with_depth(expr, 0)
    }

    fn read_origin_tree_with_depth(&mut self, expr: Expr, depth: usize) -> Result<LocatedExprTree> {
        self.check_depth(depth)?;
        let origin = match self.read_u8()? {
            0 => None,
            1 => Some(self.read_origin()?),
            other => return Err(self.error(format!("unknown tree origin presence tag {other}"))),
        };
        match expr {
            Expr::Nil
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Symbol(_)
            | Expr::Local(_)
            | Expr::String(_)
            | Expr::Bytes(_) => Ok(LocatedExprTree::without_children(expr, origin)),
            Expr::List(items) => {
                self.read_seq_tree_with_depth(items, origin, Expr::List, depth + 1)
            }
            Expr::Vector(items) => {
                self.read_seq_tree_with_depth(items, origin, Expr::Vector, depth + 1)
            }
            Expr::Set(items) => self.read_seq_tree_with_depth(items, origin, Expr::Set, depth + 1),
            Expr::Block(items) => {
                self.read_seq_tree_with_depth(items, origin, Expr::Block, depth + 1)
            }
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

    fn read_seq_tree_with_depth(
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
}
