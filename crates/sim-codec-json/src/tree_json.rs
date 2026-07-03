//! Located and tree `Expr <-> JSON` projections.
//!
//! Extends the canonical projection to carry source origins: emits and parses
//! `$located` forms for `LocatedExpr` and origin-bearing `LocatedExprTree`
//! values.

use serde_json::{Map, Value as JsonValue, json};
use sim_codec::DecodeBudget;
use sim_kernel::{
    Expr, LocatedExpr, LocatedExprTree, NumberLiteral, Origin, Result, SourceId, Span, Trivia,
};

use crate::expr_json::{expr_to_json, json_to_expr};
use crate::helpers::{
    base64_decode, base64_encode, json_error, json_to_symbol, local_to_json, optional_bytes,
    parse_quote_mode, parse_symbol, quote_mode_name, required_array, required_bool, required_str,
    required_u64, required_value, symbol_from_json, symbol_to_json,
};

/// Projects a [`LocatedExpr`] onto JSON, optionally wrapping it in a `$located`
/// form that carries its source origin when `include_origin` is set.
pub fn located_expr_to_json(located: &LocatedExpr, include_origin: bool) -> JsonValue {
    if !include_origin {
        return expr_to_json(&located.expr);
    }

    let mut object = Map::new();
    object.insert("$located".to_owned(), expr_to_json(&located.expr));
    if let Some(origin) = &located.origin {
        object.insert("origin".to_owned(), origin_to_json(origin));
    }
    JsonValue::Object(object)
}

/// Projects a [`LocatedExprTree`] onto JSON, recursively carrying per-node
/// origins as `$located` forms when `include_origin` is set.
pub fn tree_to_json(tree: &LocatedExprTree, include_origin: bool) -> JsonValue {
    tree_expr_to_json(tree, include_origin)
}

/// Reads JSON back into a [`LocatedExpr`], recovering the source origin from a
/// `$located` wrapper when present and otherwise decoding a bare expression.
pub fn json_to_located_expr(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<LocatedExpr> {
    if let Some(object) = value.as_object()
        && let Some(expr_value) = object.get("$located")
    {
        return Ok(LocatedExpr {
            expr: json_to_expr(codec, expr_value, budget, depth)?,
            origin: object
                .get("origin")
                .map(|origin| json_to_origin(codec, origin, budget))
                .transpose()?,
        });
    }

    Ok(LocatedExpr {
        expr: json_to_expr(codec, value, budget, depth)?,
        origin: None,
    })
}

/// Reads JSON back into a [`LocatedExprTree`], recovering per-node origins from
/// `$located` wrappers, under the decode `budget` and node `depth` limits.
pub fn json_to_tree(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<LocatedExprTree> {
    if let Some(object) = value.as_object()
        && let Some(expr_value) = object.get("$located")
    {
        let mut tree = json_expr_to_tree(codec, expr_value, budget, depth)?;
        tree.origin = object
            .get("origin")
            .map(|origin| json_to_origin(codec, origin, budget))
            .transpose()?;
        return Ok(tree);
    }
    json_expr_to_tree(codec, value, budget, depth)
}

fn json_expr_to_tree(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<LocatedExprTree> {
    budget.enter_node(codec, depth)?;
    let Some(tag) = value.get("$expr").and_then(JsonValue::as_str) else {
        return Err(json_error(codec, "missing $expr"));
    };

    match tag {
        "nil" => Ok(LocatedExprTree::without_children(Expr::Nil, None)),
        "bool" => Ok(LocatedExprTree::without_children(
            Expr::Bool(required_bool(codec, value, "value")?),
            None,
        )),
        "number" => Ok(LocatedExprTree::without_children(
            Expr::Number(NumberLiteral {
                domain: parse_symbol(required_str(codec, value, "domain")?),
                canonical: required_str(codec, value, "value")?.to_owned(),
            }),
            None,
        )),
        "symbol" => Ok(LocatedExprTree::without_children(
            json_to_symbol(codec, value)?,
            None,
        )),
        "local" => Ok(LocatedExprTree::without_children(
            Expr::Local(symbol_from_json(codec, value)?),
            None,
        )),
        "string" => {
            let text = required_str(codec, value, "value")?;
            budget.check_string_bytes(codec, text.len())?;
            Ok(LocatedExprTree::without_children(
                Expr::String(text.to_owned()),
                None,
            ))
        }
        "bytes" => {
            let encoded = required_str(codec, value, "base64")?;
            budget.check_blob_bytes(codec, encoded.len())?;
            let decoded = base64_decode(codec, encoded)?;
            budget.check_blob_bytes(codec, decoded.len())?;
            Ok(LocatedExprTree::without_children(
                Expr::Bytes(decoded),
                None,
            ))
        }
        "list" => tree_sequence(codec, value, "items", Expr::List, budget, depth + 1),
        "vector" => tree_sequence(codec, value, "items", Expr::Vector, budget, depth + 1),
        "map" => {
            let entries = required_array(codec, value, "entries")?;
            budget.check_collection_len(codec, entries.len())?;
            let entries = entries
                .iter()
                .map(|entry| {
                    Ok((
                        json_to_tree(
                            codec,
                            required_value(codec, entry, "key")?,
                            budget,
                            depth + 1,
                        )?,
                        json_to_tree(
                            codec,
                            required_value(codec, entry, "value")?,
                            budget,
                            depth + 1,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(LocatedExprTree {
                expr: Expr::Map(
                    entries
                        .iter()
                        .map(|(key, value)| (key.expr.clone(), value.expr.clone()))
                        .collect(),
                ),
                origin: None,
                children: entries
                    .into_iter()
                    .flat_map(|(key, value)| [key, value])
                    .collect(),
            })
        }
        "set" => tree_sequence(codec, value, "items", Expr::Set, budget, depth + 1),
        "call" => {
            let operator = json_to_tree(
                codec,
                required_value(codec, value, "operator")?,
                budget,
                depth + 1,
            )?;
            let raw_args = required_array(codec, value, "args")?;
            budget.check_collection_len(codec, raw_args.len())?;
            let args = raw_args
                .iter()
                .map(|arg| json_to_tree(codec, arg, budget, depth + 1))
                .collect::<Result<Vec<_>>>()?;
            let mut children = Vec::with_capacity(args.len() + 1);
            children.push(operator.clone());
            children.extend(args.iter().cloned());
            Ok(LocatedExprTree {
                expr: Expr::Call {
                    operator: Box::new(operator.expr.clone()),
                    args: args.iter().map(|arg| arg.expr.clone()).collect(),
                },
                origin: None,
                children,
            })
        }
        "infix" => {
            let left = json_to_tree(
                codec,
                required_value(codec, value, "left")?,
                budget,
                depth + 1,
            )?;
            let right = json_to_tree(
                codec,
                required_value(codec, value, "right")?,
                budget,
                depth + 1,
            )?;
            Ok(LocatedExprTree {
                expr: Expr::Infix {
                    operator: symbol_from_json(codec, required_value(codec, value, "operator")?)?,
                    left: Box::new(left.expr.clone()),
                    right: Box::new(right.expr.clone()),
                },
                origin: None,
                children: vec![left, right],
            })
        }
        "prefix" => {
            let arg = json_to_tree(
                codec,
                required_value(codec, value, "arg")?,
                budget,
                depth + 1,
            )?;
            Ok(LocatedExprTree {
                expr: Expr::Prefix {
                    operator: symbol_from_json(codec, required_value(codec, value, "operator")?)?,
                    arg: Box::new(arg.expr.clone()),
                },
                origin: None,
                children: vec![arg],
            })
        }
        "postfix" => {
            let arg = json_to_tree(
                codec,
                required_value(codec, value, "arg")?,
                budget,
                depth + 1,
            )?;
            Ok(LocatedExprTree {
                expr: Expr::Postfix {
                    operator: symbol_from_json(codec, required_value(codec, value, "operator")?)?,
                    arg: Box::new(arg.expr.clone()),
                },
                origin: None,
                children: vec![arg],
            })
        }
        "block" => tree_sequence(codec, value, "items", Expr::Block, budget, depth + 1),
        "quote" => {
            let expr = json_to_tree(
                codec,
                required_value(codec, value, "expr")?,
                budget,
                depth + 1,
            )?;
            Ok(LocatedExprTree {
                expr: Expr::Quote {
                    mode: parse_quote_mode(codec, required_str(codec, value, "mode")?)?,
                    expr: Box::new(expr.expr.clone()),
                },
                origin: None,
                children: vec![expr],
            })
        }
        "annotated" => {
            let expr = json_to_tree(
                codec,
                required_value(codec, value, "expr")?,
                budget,
                depth + 1,
            )?;
            let annotations = required_array(codec, value, "annotations")?;
            budget.check_collection_len(codec, annotations.len())?;
            let annotations = annotations
                .iter()
                .map(|entry| {
                    Ok((
                        symbol_from_json(codec, required_value(codec, entry, "key")?)?,
                        json_to_tree(
                            codec,
                            required_value(codec, entry, "value")?,
                            budget,
                            depth + 1,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut children = Vec::with_capacity(annotations.len() + 1);
            children.push(expr.clone());
            children.extend(annotations.iter().map(|(_, value)| value.clone()));
            Ok(LocatedExprTree {
                expr: Expr::Annotated {
                    expr: Box::new(expr.expr.clone()),
                    annotations: annotations
                        .iter()
                        .map(|(key, value)| (key.clone(), value.expr.clone()))
                        .collect(),
                },
                origin: None,
                children,
            })
        }
        "extension" => {
            let payload = json_to_tree(
                codec,
                required_value(codec, value, "payload")?,
                budget,
                depth + 1,
            )?;
            Ok(LocatedExprTree {
                expr: Expr::Extension {
                    tag: symbol_from_json(codec, required_value(codec, value, "tag")?)?,
                    payload: Box::new(payload.expr.clone()),
                },
                origin: None,
                children: vec![payload],
            })
        }
        other => Err(json_error(codec, format!("unknown expr tag {other}"))),
    }
}

fn tree_expr_to_json(tree: &LocatedExprTree, include_origin: bool) -> JsonValue {
    let expr_value = match &tree.expr {
        Expr::Nil => json!({ "$expr": "nil" }),
        Expr::Bool(value) => json!({ "$expr": "bool", "value": value }),
        Expr::Number(number) => json!({
            "$expr": "number",
            "domain": number.domain.to_string(),
            "value": number.canonical,
        }),
        Expr::Symbol(symbol) => symbol_to_json(symbol),
        Expr::Local(symbol) => local_to_json(symbol),
        Expr::String(value) => json!({ "$expr": "string", "value": value }),
        Expr::Bytes(value) => json!({ "$expr": "bytes", "base64": base64_encode(value) }),
        Expr::List(_) => tree_sequence_json("list", &tree.children, include_origin),
        Expr::Vector(_) => tree_sequence_json("vector", &tree.children, include_origin),
        Expr::Map(_) => json!({
            "$expr": "map",
            "entries": tree.children.chunks(2).map(|pair| json!({
                "key": tree_to_json(&pair[0], include_origin),
                "value": tree_to_json(&pair[1], include_origin),
            })).collect::<Vec<_>>(),
        }),
        Expr::Set(_) => tree_sequence_json("set", &tree.children, include_origin),
        Expr::Call { .. } => json!({
            "$expr": "call",
            "operator": tree_to_json(&tree.children[0], include_origin),
            "args": tree.children[1..].iter().map(|item| tree_to_json(item, include_origin)).collect::<Vec<_>>(),
        }),
        Expr::Infix { operator, .. } => json!({
            "$expr": "infix",
            "operator": symbol_to_json(operator),
            "left": tree_to_json(&tree.children[0], include_origin),
            "right": tree_to_json(&tree.children[1], include_origin),
        }),
        Expr::Prefix { operator, .. } => json!({
            "$expr": "prefix",
            "operator": symbol_to_json(operator),
            "arg": tree_to_json(&tree.children[0], include_origin),
        }),
        Expr::Postfix { operator, .. } => json!({
            "$expr": "postfix",
            "operator": symbol_to_json(operator),
            "arg": tree_to_json(&tree.children[0], include_origin),
        }),
        Expr::Block(_) => tree_sequence_json("block", &tree.children, include_origin),
        Expr::Quote { mode, .. } => json!({
            "$expr": "quote",
            "mode": quote_mode_name(*mode),
            "expr": tree_to_json(&tree.children[0], include_origin),
        }),
        Expr::Annotated { annotations, .. } => json!({
            "$expr": "annotated",
            "expr": tree_to_json(&tree.children[0], include_origin),
            "annotations": annotations.iter().zip(tree.children[1..].iter()).map(|((key, _), value)| json!({
                "key": symbol_to_json(key),
                "value": tree_to_json(value, include_origin),
            })).collect::<Vec<_>>(),
        }),
        Expr::Extension { tag, .. } => json!({
            "$expr": "extension",
            "tag": symbol_to_json(tag),
            "payload": tree_to_json(&tree.children[0], include_origin),
        }),
    };
    if include_origin && let Some(origin) = tree.origin.as_ref() {
        let mut object = Map::new();
        object.insert("$located".to_owned(), expr_value);
        object.insert("origin".to_owned(), origin_to_json(origin));
        JsonValue::Object(object)
    } else {
        expr_value
    }
}

fn tree_sequence_json(
    tag: &'static str,
    items: &[LocatedExprTree],
    include_origin: bool,
) -> JsonValue {
    json!({
        "$expr": tag,
        "items": items.iter().map(|item| tree_to_json(item, include_origin)).collect::<Vec<_>>(),
    })
}

fn tree_sequence(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    field: &str,
    build: fn(Vec<Expr>) -> Expr,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<LocatedExprTree> {
    let children = required_array(codec, value, field)?;
    budget.check_collection_len(codec, children.len())?;
    let children = children
        .iter()
        .map(|item| json_to_tree(codec, item, budget, depth))
        .collect::<Result<Vec<_>>>()?;
    Ok(LocatedExprTree {
        expr: build(children.iter().map(|item| item.expr.clone()).collect()),
        origin: None,
        children,
    })
}

fn origin_to_json(origin: &Origin) -> JsonValue {
    json!({
        "codec": origin.codec.0,
        "source": origin.source.0,
        "span": {
            "start": origin.span.start,
            "end": origin.span.end,
        },
        "trivia": origin.trivia.iter().map(trivia_to_json).collect::<Vec<_>>(),
    })
}

fn trivia_to_json(trivia: &Trivia) -> JsonValue {
    match trivia {
        Trivia::Whitespace(text) => json!({ "kind": "whitespace", "text": text }),
        Trivia::LineComment(text) => json!({ "kind": "line-comment", "text": text }),
        Trivia::BlockComment(text) => json!({ "kind": "block-comment", "text": text }),
    }
}

fn json_to_origin(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
) -> Result<Origin> {
    let trivia = required_array(codec, value, "trivia")?;
    budget.check_collection_len(codec, trivia.len())?;
    let origin = Origin {
        codec: sim_kernel::CodecId(required_u64(codec, value, "codec")? as u32),
        source: SourceId(required_str(codec, value, "source")?.to_owned()),
        span: {
            let span = required_value(codec, value, "span")?;
            Span {
                start: required_u64(codec, span, "start")? as usize,
                end: required_u64(codec, span, "end")? as usize,
            }
        },
        trivia: trivia
            .iter()
            .map(|item| json_to_trivia(codec, item, budget))
            .collect::<Result<Vec<_>>>()?,
    };
    if let Some(raw) = optional_bytes(codec, value, "raw")? {
        let mut registry = sim_kernel::SourceRegistry::default();
        registry.intern_span(&origin, &raw);
    }
    Ok(origin)
}

fn json_to_trivia(
    codec: sim_kernel::CodecId,
    value: &JsonValue,
    budget: &mut DecodeBudget,
) -> Result<Trivia> {
    budget.add_trivia(codec)?;
    let kind = required_str(codec, value, "kind")?;
    let text = required_str(codec, value, "text")?.to_owned();
    match kind {
        "whitespace" => Ok(Trivia::Whitespace(text)),
        "line-comment" => Ok(Trivia::LineComment(text)),
        "block-comment" => Ok(Trivia::BlockComment(text)),
        other => Err(json_error(codec, format!("unknown trivia kind {other}"))),
    }
}
