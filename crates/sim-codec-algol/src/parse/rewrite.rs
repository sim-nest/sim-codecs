//! Post-parse rewriting for Algol: carries raw number literals as a tagged
//! extension form and lowers them into concrete number domains via the context.

use sim_codec_pratt::{raw_number_expr, raw_number_tag};
use sim_kernel::{Expr, LocatedExprTree, Result};

pub(crate) fn rewrite_number_domains_tree_lossy(
    cx: &mut sim_kernel::Cx,
    tree: &mut LocatedExprTree,
) -> Result<()> {
    tree.expr = rewrite_number_domains_expr_lossy(cx, tree.expr.clone())?;
    if matches!(tree.expr, Expr::Quote { .. }) {
        return Ok(());
    }
    for child in &mut tree.children {
        rewrite_number_domains_tree_lossy(cx, child)?;
    }
    Ok(())
}

fn rewrite_number_domains_expr_lossy(cx: &mut sim_kernel::Cx, expr: Expr) -> Result<Expr> {
    Ok(match expr {
        Expr::Extension { tag, payload } if tag == raw_number_tag() => {
            let Expr::String(raw) = *payload else {
                return Ok(Expr::Extension { tag, payload });
            };
            match cx.parse_number_literal(&raw)? {
                Some(number) => Expr::Number(number),
                None => raw_number_expr(raw),
            }
        }
        Expr::List(items) => Expr::List(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr_lossy(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Vector(items) => Expr::Vector(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr_lossy(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Map(entries) => Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    Ok((
                        rewrite_number_domains_expr_lossy(cx, key)?,
                        rewrite_number_domains_expr_lossy(cx, value)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Set(items) => Expr::Set(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr_lossy(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Call { operator, args } => Expr::Call {
            operator: Box::new(rewrite_number_domains_expr_lossy(cx, *operator)?),
            args: args
                .into_iter()
                .map(|arg| rewrite_number_domains_expr_lossy(cx, arg))
                .collect::<Result<Vec<_>>>()?,
        },
        Expr::Infix {
            operator,
            left,
            right,
        } => Expr::Infix {
            operator,
            left: Box::new(rewrite_number_domains_expr_lossy(cx, *left)?),
            right: Box::new(rewrite_number_domains_expr_lossy(cx, *right)?),
        },
        Expr::Prefix { operator, arg } => Expr::Prefix {
            operator,
            arg: Box::new(rewrite_number_domains_expr_lossy(cx, *arg)?),
        },
        Expr::Postfix { operator, arg } => Expr::Postfix {
            operator,
            arg: Box::new(rewrite_number_domains_expr_lossy(cx, *arg)?),
        },
        Expr::Block(items) => Expr::Block(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr_lossy(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Quote { mode, expr } => Expr::Quote { mode, expr },
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(rewrite_number_domains_expr_lossy(cx, *expr)?),
            annotations: annotations
                .into_iter()
                .map(|(name, value)| Ok((name, rewrite_number_domains_expr_lossy(cx, value)?)))
                .collect::<Result<Vec<_>>>()?,
        },
        Expr::Extension { tag, payload } => Expr::Extension {
            tag,
            payload: Box::new(rewrite_number_domains_expr_lossy(cx, *payload)?),
        },
        other => other,
    })
}
