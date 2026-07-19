use sim_codec::{DecodeBudget, Input, ReadCx};
use sim_kernel::{
    Expr, LocatedExpr, LocatedExprTree, NumberLiteral, Origin, Result, SourceId, Span, Symbol,
};

use crate::ast::{
    LuaBinOp, LuaBlock, LuaExpr, LuaField, LuaFuncBody, LuaFunctionName, LuaLocalAttr, LuaStmt,
    LuaUnOp,
};
use crate::parse_lua_chunk_with_budget;

/// Decodes one Lua chunk into the `lua/*` expression surface.
pub fn decode_lua_chunk(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<Expr> {
    let _ = source_id.into();
    let chunk = parse_lua_chunk_with_budget(cx.codec, source, budget)?;
    Ok(lower_lua_chunk(&chunk))
}

/// Decodes one Lua chunk into a root-located expression.
pub fn decode_lua_located_chunk(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExpr> {
    let source = input.into_string_for(cx.codec)?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let mut budget = DecodeBudget::new(cx.limits);
    budget.check_input_bytes(cx.codec, source.len())?;
    let expr = decode_lua_chunk(cx, source_id.0.clone(), &source, &mut budget)?;
    Ok(LocatedExpr {
        expr,
        origin: Some(full_origin(cx.codec, source_id, &source)),
    })
}

/// Decodes one Lua chunk into a located tree with the chunk root spanning the source.
pub fn decode_lua_tree_chunk(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExprTree> {
    let located = decode_lua_located_chunk(cx, source_id, input)?;
    let mut tree = LocatedExprTree::from_expr_recursive(located.expr);
    tree.origin = located.origin;
    Ok(tree)
}

/// Lowers a parsed Lua chunk to a `lua/chunk` expression.
pub fn lower_lua_chunk(chunk: &LuaBlock) -> Expr {
    call("chunk", chunk.statements.iter().map(lower_stmt).collect())
}

fn lower_block(block: &LuaBlock) -> Expr {
    call("block", block.statements.iter().map(lower_stmt).collect())
}

fn lower_stmt(stmt: &LuaStmt) -> Expr {
    match stmt {
        LuaStmt::Local { bindings, values } => call(
            "local",
            vec![
                Expr::Vector(bindings.iter().map(lower_binding).collect()),
                Expr::Vector(values.iter().map(lower_expr).collect()),
            ],
        ),
        LuaStmt::Assign { targets, values } => call(
            "assign",
            vec![
                Expr::Vector(targets.iter().map(lower_expr).collect()),
                Expr::Vector(values.iter().map(lower_expr).collect()),
            ],
        ),
        LuaStmt::If { arms, else_block } => {
            let mut args = Vec::new();
            for (index, arm) in arms.iter().enumerate() {
                if index == 0 {
                    args.push(lower_expr(&arm.condition));
                    args.push(lower_block(&arm.block));
                } else {
                    args.push(call(
                        "elseif",
                        vec![lower_expr(&arm.condition), lower_block(&arm.block)],
                    ));
                }
            }
            if let Some(block) = else_block {
                args.push(call("else", vec![lower_block(block)]));
            }
            call("if", args)
        }
        LuaStmt::While { condition, block } => {
            call("while", vec![lower_expr(condition), lower_block(block)])
        }
        LuaStmt::Repeat { block, condition } => {
            call("repeat", vec![lower_block(block), lower_expr(condition)])
        }
        LuaStmt::NumericFor {
            name,
            start,
            limit,
            step,
            block,
        } => call(
            "for-range",
            vec![
                Expr::Symbol(name.clone()),
                lower_expr(start),
                lower_expr(limit),
                step.as_ref().map(lower_expr).unwrap_or(Expr::Nil),
                lower_block(block),
            ],
        ),
        LuaStmt::GenericFor { names, iter, block } => call(
            "for-in",
            vec![
                Expr::List(names.iter().cloned().map(Expr::Symbol).collect()),
                Expr::Vector(iter.iter().map(lower_expr).collect()),
                lower_block(block),
            ],
        ),
        LuaStmt::Function { name, body } => call(
            "function-decl",
            vec![
                lower_function_name(name),
                lower_params(body),
                lower_block(&body.block),
            ],
        ),
        LuaStmt::LocalFunction { name, body } => call(
            "local-function",
            vec![
                Expr::Symbol(name.clone()),
                lower_params(body),
                lower_block(&body.block),
            ],
        ),
        LuaStmt::Return(values) => call("return", values.iter().map(lower_expr).collect()),
        LuaStmt::Break => call("break", Vec::new()),
        LuaStmt::Label(name) => call("label", vec![Expr::Symbol(name.clone())]),
        LuaStmt::Goto(name) => call("goto", vec![Expr::Symbol(name.clone())]),
        LuaStmt::Do(block) => call("do", vec![lower_block(block)]),
        LuaStmt::Expr(expr) => call("expr", vec![lower_expr(expr)]),
    }
}

fn lower_binding(binding: &crate::LuaBinding) -> Expr {
    match binding.attr {
        Some(attr) => call(
            "binding",
            vec![
                Expr::Symbol(binding.name.clone()),
                Expr::Symbol(Symbol::new(match attr {
                    LuaLocalAttr::Const => "const",
                    LuaLocalAttr::Close => "close",
                })),
            ],
        ),
        None => Expr::Symbol(binding.name.clone()),
    }
}

fn lower_function_name(name: &LuaFunctionName) -> Expr {
    if name.fields.is_empty() && name.method.is_none() {
        return Expr::Symbol(name.base.clone());
    }
    call(
        "name",
        vec![
            Expr::Symbol(name.base.clone()),
            Expr::Vector(name.fields.iter().cloned().map(Expr::Symbol).collect()),
            name.method
                .as_ref()
                .map(|method| Expr::Symbol(method.clone()))
                .unwrap_or(Expr::Nil),
        ],
    )
}

fn lower_params(body: &LuaFuncBody) -> Expr {
    let mut params = body
        .params
        .iter()
        .cloned()
        .map(Expr::Symbol)
        .collect::<Vec<_>>();
    if body.vararg {
        params.push(call("vararg", Vec::new()));
    }
    Expr::List(params)
}

fn lower_expr(expr: &LuaExpr) -> Expr {
    match expr {
        LuaExpr::Nil => Expr::Nil,
        LuaExpr::True => Expr::Bool(true),
        LuaExpr::False => Expr::Bool(false),
        LuaExpr::Number(raw) => Expr::Number(NumberLiteral {
            domain: Symbol::qualified("lua", "number"),
            canonical: raw.clone(),
        }),
        LuaExpr::Str(text) => Expr::String(text.clone()),
        LuaExpr::Vararg => call("vararg", Vec::new()),
        LuaExpr::Name(name) => Expr::Symbol(name.clone()),
        LuaExpr::Index { obj, key } => call("index", vec![lower_expr(obj), lower_expr(key)]),
        LuaExpr::Call { callee, args } => {
            let mut lowered = vec![lower_expr(callee)];
            lowered.extend(args.iter().map(lower_expr));
            call("call", lowered)
        }
        LuaExpr::Method { recv, name, args } => {
            let mut lowered = vec![lower_expr(recv), Expr::Symbol(name.clone())];
            lowered.extend(args.iter().map(lower_expr));
            call("method-call", lowered)
        }
        LuaExpr::Unary { op, rhs } => call(unary_name(*op), vec![lower_expr(rhs)]),
        LuaExpr::Binary { op, lhs, rhs } => {
            call(binary_name(*op), vec![lower_expr(lhs), lower_expr(rhs)])
        }
        LuaExpr::Table(fields) => call("table", fields.iter().map(lower_field).collect()),
        LuaExpr::Function(body) => call(
            "function",
            vec![lower_params(body), lower_block(&body.block)],
        ),
    }
}

fn lower_field(field: &LuaField) -> Expr {
    match field {
        LuaField::Positional(value) => call("field", vec![lower_expr(value)]),
        LuaField::Named { key, value } => call(
            "named-field",
            vec![Expr::Symbol(key.clone()), lower_expr(value)],
        ),
        LuaField::Keyed { key, value } => {
            call("keyed-field", vec![lower_expr(key), lower_expr(value)])
        }
    }
}

fn binary_name(op: LuaBinOp) -> &'static str {
    match op {
        LuaBinOp::Or => "or",
        LuaBinOp::And => "and",
        LuaBinOp::Lt => "lt",
        LuaBinOp::Gt => "gt",
        LuaBinOp::Le => "le",
        LuaBinOp::Ge => "ge",
        LuaBinOp::Ne => "ne",
        LuaBinOp::Eq => "eq",
        LuaBinOp::BitOr => "bit-or",
        LuaBinOp::BitXor => "bit-xor",
        LuaBinOp::BitAnd => "bit-and",
        LuaBinOp::Shl => "shl",
        LuaBinOp::Shr => "shr",
        LuaBinOp::Concat => "concat",
        LuaBinOp::Add => "add",
        LuaBinOp::Sub => "sub",
        LuaBinOp::Mul => "mul",
        LuaBinOp::Div => "div",
        LuaBinOp::FloorDiv => "floor-div",
        LuaBinOp::Mod => "mod",
        LuaBinOp::Pow => "pow",
    }
}

fn unary_name(op: LuaUnOp) -> &'static str {
    match op {
        LuaUnOp::Not => "not",
        LuaUnOp::Len => "len",
        LuaUnOp::Neg => "neg",
        LuaUnOp::BitNot => "bit-not",
    }
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("lua", name))),
        args,
    }
}

fn full_origin(codec: sim_kernel::CodecId, source: SourceId, text: &str) -> Origin {
    Origin {
        codec,
        source,
        span: Span {
            start: 0,
            end: text.len(),
        },
        trivia: Vec::new(),
    }
}
