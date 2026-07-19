use sim_codec::DecodeBudget;
use sim_kernel::{
    CodecId, Error, Expr, LocatedExprTree, PrattOperator, PrattResult, PrattToken as Token, Result,
    SourceId,
};

use crate::parser::{ParseCx, extend_tree_trivia, tree_origin};
use crate::{PrattCodecParser, PrattTokenSource, SpannedPrattToken};

impl<S: PrattTokenSource> PrattCodecParser<S> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_postfix_tree(
        &self,
        codec: CodecId,
        source_id: &SourceId,
        source: &str,
        left: LocatedExprTree,
        op: PrattOperator,
        end: usize,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        budget.enter_node(codec, depth)?;
        let start = left
            .origin
            .as_ref()
            .map(|origin| origin.span.start)
            .unwrap_or(0);
        let origin = tree_origin(
            codec,
            source_id.clone(),
            source,
            start,
            end,
            left.origin
                .as_ref()
                .map(|origin| origin.trivia.clone())
                .unwrap_or_default(),
        );
        match op.result {
            PrattResult::ExprPostfix => Ok(LocatedExprTree {
                expr: Expr::Postfix {
                    operator: op.symbol,
                    arg: Box::new(left.expr.clone()),
                },
                origin: Some(origin),
                children: vec![left],
            }),
            PrattResult::Call(symbol) => Ok(LocatedExprTree {
                expr: Expr::Call {
                    operator: Box::new(Expr::Symbol(symbol)),
                    args: vec![left.expr.clone()],
                },
                origin: Some(origin),
                children: vec![left],
            }),
            PrattResult::Custom(tag) => Ok(LocatedExprTree {
                expr: Expr::Extension {
                    tag,
                    payload: Box::new(Expr::List(vec![left.expr.clone()])),
                },
                origin: Some(origin),
                children: vec![left],
            }),
            _ => Err(Error::Eval("invalid postfix operator result".to_owned())),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_call_tree(
        &self,
        cx: &mut ParseCx,
        codec: CodecId,
        source_id: &SourceId,
        source: &str,
        mut operator: LocatedExprTree,
        open_start: usize,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        let mut args = Vec::new();
        if matches!(
            cx.peek(),
            Some(SpannedPrattToken {
                token: Token::CloseParen,
                ..
            })
        ) {
            let close = cx.next_required()?;
            let end = close.end;
            let parent_trivia = close.leading_trivia.clone();
            extend_tree_trivia(&mut operator, close.leading_trivia);
            let expr = Expr::Call {
                operator: Box::new(operator.expr.clone()),
                args: Vec::new(),
            };
            return Ok(LocatedExprTree {
                expr,
                origin: Some(tree_origin(
                    codec,
                    source_id.clone(),
                    source,
                    operator
                        .origin
                        .as_ref()
                        .map(|origin| origin.span.start)
                        .unwrap_or(open_start),
                    end,
                    parent_trivia,
                )),
                children: vec![operator],
            });
        }

        let end;
        loop {
            budget.check_collection_len(codec, args.len() + 1)?;
            args.push(self.parse_expr_tree(cx, codec, source_id, source, 0, budget, depth + 1)?);
            let delimiter = cx.next_required()?;
            match delimiter.token {
                Token::Comma => {
                    if let Some(last) = args.last_mut() {
                        extend_tree_trivia(last, delimiter.leading_trivia);
                    }
                    continue;
                }
                Token::CloseParen => {
                    if let Some(last) = args.last_mut() {
                        extend_tree_trivia(last, delimiter.leading_trivia);
                    }
                    end = delimiter.end;
                    break;
                }
                other => {
                    return Err(Error::Eval(format!(
                        "expected ',' or ')' in call, found {:?}",
                        other
                    )));
                }
            }
        }

        let mut children = Vec::with_capacity(args.len() + 1);
        children.push(operator.clone());
        children.extend(args.iter().cloned());
        let mut parent_trivia = Vec::new();
        if let Some(last) = args.last()
            && let Some(origin) = &last.origin
        {
            parent_trivia.extend(origin.trivia.clone());
        }
        budget.enter_node(codec, depth)?;
        Ok(LocatedExprTree {
            expr: Expr::Call {
                operator: Box::new(operator.expr.clone()),
                args: args.iter().map(|arg| arg.expr.clone()).collect(),
            },
            origin: Some(tree_origin(
                codec,
                source_id.clone(),
                source,
                operator
                    .origin
                    .as_ref()
                    .map(|origin| origin.span.start)
                    .unwrap_or(open_start),
                end,
                parent_trivia,
            )),
            children,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_prefix_tree(
        &self,
        codec: CodecId,
        source_id: &SourceId,
        source: &str,
        op: PrattOperator,
        start: usize,
        right: LocatedExprTree,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        budget.enter_node(codec, depth)?;
        let end = right
            .origin
            .as_ref()
            .map(|origin| origin.span.end)
            .unwrap_or(start);
        let origin = tree_origin(
            codec,
            source_id.clone(),
            source,
            start,
            end,
            right
                .origin
                .as_ref()
                .map(|origin| origin.trivia.clone())
                .unwrap_or_default(),
        );
        match op.result {
            PrattResult::ExprPrefix => Ok(LocatedExprTree {
                expr: Expr::Prefix {
                    operator: op.symbol,
                    arg: Box::new(right.expr.clone()),
                },
                origin: Some(origin),
                children: vec![right],
            }),
            PrattResult::Call(symbol) => Ok(LocatedExprTree {
                expr: Expr::Call {
                    operator: Box::new(Expr::Symbol(symbol)),
                    args: vec![right.expr.clone()],
                },
                origin: Some(origin),
                children: vec![right],
            }),
            PrattResult::Custom(tag) => Ok(LocatedExprTree {
                expr: Expr::Extension {
                    tag,
                    payload: Box::new(Expr::List(vec![right.expr.clone()])),
                },
                origin: Some(origin),
                children: vec![right],
            }),
            _ => Err(Error::Eval("invalid prefix operator result".to_owned())),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_infix_tree(
        &self,
        codec: CodecId,
        source_id: &SourceId,
        source: &str,
        op: PrattOperator,
        left: LocatedExprTree,
        right: LocatedExprTree,
        budget: &mut DecodeBudget,
        depth: usize,
    ) -> Result<LocatedExprTree> {
        budget.enter_node(codec, depth)?;
        let start = left
            .origin
            .as_ref()
            .map(|origin| origin.span.start)
            .unwrap_or(0);
        let end = right
            .origin
            .as_ref()
            .map(|origin| origin.span.end)
            .unwrap_or(start);
        let origin = tree_origin(
            codec,
            source_id.clone(),
            source,
            start,
            end,
            right
                .origin
                .as_ref()
                .map(|origin| origin.trivia.clone())
                .unwrap_or_default(),
        );
        match op.result {
            PrattResult::ExprInfix => Ok(LocatedExprTree {
                expr: Expr::Infix {
                    operator: op.symbol,
                    left: Box::new(left.expr.clone()),
                    right: Box::new(right.expr.clone()),
                },
                origin: Some(origin),
                children: vec![left, right],
            }),
            PrattResult::Call(symbol) => Ok(LocatedExprTree {
                expr: Expr::Call {
                    operator: Box::new(Expr::Symbol(symbol)),
                    args: vec![left.expr.clone(), right.expr.clone()],
                },
                origin: Some(origin),
                children: vec![left, right],
            }),
            PrattResult::Custom(tag) => Ok(LocatedExprTree {
                expr: Expr::Extension {
                    tag,
                    payload: Box::new(Expr::List(vec![left.expr.clone(), right.expr.clone()])),
                },
                origin: Some(origin),
                children: vec![left, right],
            }),
            _ => Err(Error::Eval("invalid infix operator result".to_owned())),
        }
    }
}
