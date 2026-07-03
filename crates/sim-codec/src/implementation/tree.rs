//! Structural validation of a `LocatedExprTree`.
//!
//! Walks a located expr tree and checks that each node's child count and shape
//! match its `Expr` variant, reporting any mismatch as a codec error.

use sim_kernel::{CodecId, Error, Expr, LocatedExprTree, Result};

/// Check that a [`LocatedExprTree`] is structurally consistent with its `Expr`.
///
/// Walks the tree and verifies that every node's child count and child
/// expressions match its `Expr` variant, returning a codec error tagged `codec`
/// on the first mismatch. A [`TreeDecoder`](crate::TreeDecoder) can run this to
/// self-check its output before returning it.
pub fn validate_expr_tree(codec: CodecId, tree: &LocatedExprTree) -> Result<()> {
    fn error(codec: CodecId, message: impl Into<String>) -> Error {
        Error::CodecError {
            codec,
            message: message.into(),
        }
    }

    fn expect_children(
        codec: CodecId,
        tree: &LocatedExprTree,
        expected: usize,
        kind: &str,
    ) -> Result<()> {
        if tree.children.len() != expected {
            return Err(error(
                codec,
                format!(
                    "{kind} tree expected {expected} children, found {}",
                    tree.children.len()
                ),
            ));
        }
        Ok(())
    }

    fn validate(codec: CodecId, tree: &LocatedExprTree) -> Result<()> {
        match &tree.expr {
            Expr::Nil
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Symbol(_)
            | Expr::Local(_)
            | Expr::String(_)
            | Expr::Bytes(_) => expect_children(codec, tree, 0, "atomic")?,
            Expr::List(items) => {
                if items.len() != tree.children.len() {
                    return Err(error(codec, "list tree child count does not match expr"));
                }
                for (item, child) in items.iter().zip(tree.children.iter()) {
                    if item != &child.expr {
                        return Err(error(
                            codec,
                            "list tree child expr does not match expr item",
                        ));
                    }
                    validate(codec, child)?;
                }
            }
            Expr::Vector(items) => {
                if items.len() != tree.children.len() {
                    return Err(error(codec, "vector tree child count does not match expr"));
                }
                for (item, child) in items.iter().zip(tree.children.iter()) {
                    if item != &child.expr {
                        return Err(error(
                            codec,
                            "vector tree child expr does not match expr item",
                        ));
                    }
                    validate(codec, child)?;
                }
            }
            Expr::Map(entries) => {
                if tree.children.len() != entries.len() * 2 {
                    return Err(error(codec, "map tree child count does not match expr"));
                }
                for ((key, value), pair) in entries.iter().zip(tree.children.chunks_exact(2)) {
                    if key != &pair[0].expr || value != &pair[1].expr {
                        return Err(error(
                            codec,
                            "map tree child expr does not match expr entry",
                        ));
                    }
                    validate(codec, &pair[0])?;
                    validate(codec, &pair[1])?;
                }
            }
            Expr::Set(items) => {
                if items.len() != tree.children.len() {
                    return Err(error(codec, "set tree child count does not match expr"));
                }
                for (item, child) in items.iter().zip(tree.children.iter()) {
                    if item != &child.expr {
                        return Err(error(codec, "set tree child expr does not match expr item"));
                    }
                    validate(codec, child)?;
                }
            }
            Expr::Call { operator, args } => {
                expect_children(codec, tree, args.len() + 1, "call")?;
                if operator.as_ref() != &tree.children[0].expr {
                    return Err(error(codec, "call tree operator child does not match expr"));
                }
                validate(codec, &tree.children[0])?;
                for (arg, child) in args.iter().zip(tree.children[1..].iter()) {
                    if arg != &child.expr {
                        return Err(error(codec, "call tree arg child does not match expr"));
                    }
                    validate(codec, child)?;
                }
            }
            Expr::Infix { left, right, .. } => {
                expect_children(codec, tree, 2, "infix")?;
                if left.as_ref() != &tree.children[0].expr
                    || right.as_ref() != &tree.children[1].expr
                {
                    return Err(error(codec, "infix tree children do not match expr"));
                }
                validate(codec, &tree.children[0])?;
                validate(codec, &tree.children[1])?;
            }
            Expr::Prefix { arg, .. } => {
                expect_children(codec, tree, 1, "prefix")?;
                if arg.as_ref() != &tree.children[0].expr {
                    return Err(error(codec, "prefix tree child does not match expr"));
                }
                validate(codec, &tree.children[0])?;
            }
            Expr::Postfix { arg, .. } => {
                expect_children(codec, tree, 1, "postfix")?;
                if arg.as_ref() != &tree.children[0].expr {
                    return Err(error(codec, "postfix tree child does not match expr"));
                }
                validate(codec, &tree.children[0])?;
            }
            Expr::Block(items) => {
                if items.len() != tree.children.len() {
                    return Err(error(codec, "block tree child count does not match expr"));
                }
                for (item, child) in items.iter().zip(tree.children.iter()) {
                    if item != &child.expr {
                        return Err(error(
                            codec,
                            "block tree child expr does not match expr item",
                        ));
                    }
                    validate(codec, child)?;
                }
            }
            Expr::Quote { expr, .. } => {
                expect_children(codec, tree, 1, "quote")?;
                if expr.as_ref() != &tree.children[0].expr {
                    return Err(error(codec, "quote tree child does not match expr"));
                }
                validate(codec, &tree.children[0])?;
            }
            Expr::Annotated { expr, annotations } => {
                expect_children(codec, tree, annotations.len() + 1, "annotated")?;
                if expr.as_ref() != &tree.children[0].expr {
                    return Err(error(codec, "annotated expr child does not match expr"));
                }
                validate(codec, &tree.children[0])?;
                for ((_, value), child) in annotations.iter().zip(tree.children[1..].iter()) {
                    if value != &child.expr {
                        return Err(error(
                            codec,
                            "annotated value child does not match expr annotation",
                        ));
                    }
                    validate(codec, child)?;
                }
            }
            Expr::Extension { payload, .. } => {
                expect_children(codec, tree, 1, "extension")?;
                if payload.as_ref() != &tree.children[0].expr {
                    return Err(error(codec, "extension tree child does not match expr"));
                }
                validate(codec, &tree.children[0])?;
            }
        }
        Ok(())
    }

    validate(codec, tree)
}
