use sim_codec::{
    CodecRuntime, DecodeBudget, DecodeLimits, Input, decode_located_with_codec,
    decode_tree_with_codec, decode_with_codec, encode_tree_with_codec, encode_with_codec,
};
use sim_kernel::{CodecId, EncodeOptions, Error, Expr, ReadPolicy, SourceId, Symbol};

use crate::{
    LuaBinOp, LuaExpr, LuaField, LuaLocalAttr, LuaStmt, LuaTokenKind, LuaUnOp, parse_lua_chunk,
    parse_lua_expr, parse_lua_expr_tree, parse_lua_expr_with_budget, tokenize_lua,
};

#[test]
fn lexes_long_strings_hex_floats_and_comments() {
    let tokens = tokenize_lua("-- note\n[=[long]=] 0x1.8p+1 --[[block]] name").unwrap();

    assert!(tokens[0].leading_trivia.iter().any(
        |trivia| matches!(trivia, sim_kernel::Trivia::LineComment(text) if text.contains("note"))
    ));
    assert_eq!(tokens[0].kind, LuaTokenKind::String("long".to_owned()));
    assert_eq!(tokens[1].kind, LuaTokenKind::Number("0x1.8p+1".to_owned()));
    assert!(tokens[2].leading_trivia.iter().any(
        |trivia| matches!(trivia, sim_kernel::Trivia::BlockComment(text) if text.contains("block"))
    ));
}

#[test]
fn parses_arithmetic_precedence() {
    let expr = parse_lua_expr("1 + 2 * 3").unwrap();
    let LuaExpr::Binary {
        op: LuaBinOp::Add,
        rhs,
        ..
    } = expr
    else {
        panic!("expected addition");
    };
    assert!(matches!(
        rhs.as_ref(),
        LuaExpr::Binary {
            op: LuaBinOp::Mul,
            ..
        }
    ));
}

#[test]
fn parses_logical_precedence() {
    let expr = parse_lua_expr("a and b or c").unwrap();
    let LuaExpr::Binary {
        op: LuaBinOp::Or,
        lhs,
        ..
    } = expr
    else {
        panic!("expected or");
    };
    assert!(matches!(
        lhs.as_ref(),
        LuaExpr::Binary {
            op: LuaBinOp::And,
            ..
        }
    ));
}

#[test]
fn parses_unary_and_power_order() {
    let expr = parse_lua_expr("-2^2 == -4").unwrap();
    let LuaExpr::Binary {
        op: LuaBinOp::Eq,
        lhs,
        rhs,
    } = expr
    else {
        panic!("expected equality");
    };
    assert!(matches!(
        lhs.as_ref(),
        LuaExpr::Unary {
            op: LuaUnOp::Neg,
            rhs,
        } if matches!(
            rhs.as_ref(),
            LuaExpr::Binary {
                op: LuaBinOp::Pow,
                ..
            }
        )
    ));
    assert!(matches!(
        rhs.as_ref(),
        LuaExpr::Unary {
            op: LuaUnOp::Neg,
            ..
        }
    ));
}

#[test]
fn parses_concat_as_right_associative() {
    let expr = parse_lua_expr("\"x\" .. y .. z").unwrap();
    let LuaExpr::Binary {
        op: LuaBinOp::Concat,
        rhs,
        ..
    } = expr
    else {
        panic!("expected concat");
    };
    assert!(matches!(
        rhs.as_ref(),
        LuaExpr::Binary {
            op: LuaBinOp::Concat,
            ..
        }
    ));
}

#[test]
fn parses_table_fields() {
    let expr = parse_lua_expr("{1, 2, k = 3, [f()] = 4}").unwrap();
    let LuaExpr::Table(fields) = expr else {
        panic!("expected table");
    };
    assert_eq!(fields.len(), 4);
    assert!(matches!(fields[0], LuaField::Positional(_)));
    assert!(matches!(&fields[2], LuaField::Named { key, .. } if key == &Symbol::new("k")));
    assert!(
        matches!(&fields[3], LuaField::Keyed { key, .. } if matches!(key, LuaExpr::Call { .. }))
    );
}

#[test]
fn parses_field_method_and_call_suffixes() {
    let expr = parse_lua_expr("t.a:m(1)").unwrap();
    let LuaExpr::Method { recv, name, args } = expr else {
        panic!("expected method call");
    };
    assert_eq!(name, Symbol::new("m"));
    assert_eq!(args.len(), 1);
    assert!(matches!(
        recv.as_ref(),
        LuaExpr::Index {
            obj,
            key,
        } if matches!(obj.as_ref(), LuaExpr::Name(name) if name == &Symbol::new("t"))
            && matches!(key.as_ref(), LuaExpr::Str(text) if text == "a")
    ));
}

#[test]
fn shared_pratt_tree_uses_lua_tokens_and_table() {
    let tree = parse_lua_expr_tree("expr.lua", "1 + 2 * 3").unwrap();
    let Expr::Infix {
        operator, right, ..
    } = tree.expr
    else {
        panic!("expected infix tree");
    };
    assert_eq!(operator, Symbol::new("+"));
    assert!(matches!(
        right.as_ref(),
        Expr::Infix { operator, .. } if operator == &Symbol::new("*")
    ));
}

#[test]
fn decode_budget_limits_tokens() {
    let mut budget = DecodeBudget::new(DecodeLimits {
        max_tokens: 2,
        ..DecodeLimits::default()
    });
    let err = parse_lua_expr_with_budget(CodecId(7), "1 + 2 * 3", &mut budget).unwrap_err();
    assert!(matches!(err, Error::CodecError { message, .. } if message.contains("tokens")));
}

#[test]
fn parses_chunk_statement_forms() {
    let chunk = parse_lua_chunk(
        r#"
local x <const>, y <close> = 1, 2
a, b = b, a
while x < 10 do x = x + 1 end
repeat x = x - 1 until x == 0
for i = 1, 3, 1 do break end
for k, v in pairs(t) do goto done end
function t.a:b(x) return x end
do ::done:: end
"#,
    )
    .unwrap();

    assert_eq!(chunk.statements.len(), 8);
    let LuaStmt::Local { bindings, .. } = &chunk.statements[0] else {
        panic!("expected local");
    };
    assert_eq!(bindings[0].attr, Some(LuaLocalAttr::Const));
    assert_eq!(bindings[1].attr, Some(LuaLocalAttr::Close));
    assert!(matches!(chunk.statements[1], LuaStmt::Assign { .. }));
    assert!(matches!(chunk.statements[2], LuaStmt::While { .. }));
    assert!(matches!(chunk.statements[3], LuaStmt::Repeat { .. }));
    assert!(matches!(chunk.statements[4], LuaStmt::NumericFor { .. }));
    assert!(matches!(chunk.statements[5], LuaStmt::GenericFor { .. }));
    assert!(matches!(chunk.statements[6], LuaStmt::Function { .. }));
    assert!(matches!(chunk.statements[7], LuaStmt::Do(_)));
}

#[test]
fn factorial_chunk_lowers_to_documented_heads() {
    let expr = crate::lower_lua_chunk(&parse_lua_chunk(FACTORIAL).unwrap());

    assert_eq!(lua_head(&expr), Some("chunk"));
    assert!(contains_lua_head(&expr, "local-function"));
    assert!(contains_lua_head(&expr, "if"));
    assert!(contains_lua_head(&expr, "eq"));
    assert!(contains_lua_head(&expr, "mul"));
    assert!(contains_lua_head(&expr, "sub"));
    assert!(contains_lua_head(&expr, "call"));
    assert!(contains_lua_head(&expr, "return"));
}

#[test]
fn lua_codec_decodes_encodes_and_preserves_tree_source() {
    let mut cx = lua_cx();
    let symbol = Symbol::qualified("codec", "lua");

    let expr = decode_with_codec(
        &mut cx,
        &symbol,
        Input::Text(FACTORIAL.to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    let encoded = encode_with_codec(&mut cx, &symbol, &expr, EncodeOptions::default())
        .unwrap()
        .into_text()
        .unwrap();
    let reparsed = decode_with_codec(
        &mut cx,
        &symbol,
        Input::Text(encoded),
        ReadPolicy::default(),
    )
    .unwrap();
    assert!(reparsed.canonical_eq(&expr));

    let located = decode_located_with_codec(
        &mut cx,
        &symbol,
        Input::Text(FACTORIAL.to_owned()),
        ReadPolicy::default(),
        "fact.lua",
    )
    .unwrap();
    let origin = located.origin.unwrap();
    assert_eq!(origin.source, SourceId("fact.lua".to_owned()));
    assert_eq!(origin.span.start, 0);
    assert_eq!(origin.span.end, FACTORIAL.len());

    let tree = decode_tree_with_codec(
        &mut cx,
        &symbol,
        Input::Text(FACTORIAL.to_owned()),
        ReadPolicy::default(),
        "fact-tree.lua",
    )
    .unwrap();
    let replayed = encode_tree_with_codec(
        &mut cx,
        &symbol,
        &tree,
        EncodeOptions {
            lossless_origin: true,
            ..EncodeOptions::default()
        },
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(replayed, FACTORIAL);
}

#[test]
fn invalid_utf8_input_reports_lua_codec_id() {
    let mut cx = lua_cx();
    let symbol = Symbol::qualified("codec", "lua");
    let expected = codec_id(&mut cx, &symbol);

    let err = decode_with_codec(
        &mut cx,
        &symbol,
        Input::Bytes(vec![0xff]),
        ReadPolicy::default(),
    )
    .unwrap_err();

    match err {
        Error::CodecError { codec, message } => {
            assert_eq!(codec, expected);
            assert_ne!(codec, CodecId(0));
            assert!(message.contains("not valid UTF-8"), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

const FACTORIAL: &str = r#"local function fact(n)
  if n == 0 then return 1 end
  return n * fact(n - 1)
end
return fact(5)"#;

fn lua_cx() -> sim_kernel::Cx {
    let mut cx = sim_test_support::core_cx();
    let lib = crate::LuaCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

fn codec_id(cx: &mut sim_kernel::Cx, symbol: &Symbol) -> CodecId {
    cx.resolve_codec(symbol)
        .unwrap()
        .object()
        .as_any()
        .downcast_ref::<CodecRuntime>()
        .unwrap()
        .id
}

fn lua_head(expr: &Expr) -> Option<&str> {
    let Expr::Call { operator, .. } = expr else {
        return None;
    };
    let Expr::Symbol(symbol) = operator.as_ref() else {
        return None;
    };
    (symbol
        .namespace
        .as_ref()
        .map(|namespace| namespace.as_ref())
        == Some("lua"))
    .then_some(symbol.name.as_ref())
}

fn contains_lua_head(expr: &Expr, head: &str) -> bool {
    lua_head(expr) == Some(head)
        || match expr {
            Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
                items.iter().any(|item| contains_lua_head(item, head))
            }
            Expr::Map(entries) => entries
                .iter()
                .any(|(key, value)| contains_lua_head(key, head) || contains_lua_head(value, head)),
            Expr::Call { operator, args } => {
                contains_lua_head(operator, head)
                    || args.iter().any(|arg| contains_lua_head(arg, head))
            }
            Expr::Infix { left, right, .. } => {
                contains_lua_head(left, head) || contains_lua_head(right, head)
            }
            Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => contains_lua_head(arg, head),
            Expr::Quote { expr, .. } => contains_lua_head(expr, head),
            Expr::Annotated { expr, annotations } => {
                contains_lua_head(expr, head)
                    || annotations
                        .iter()
                        .any(|(_, value)| contains_lua_head(value, head))
            }
            Expr::Extension { payload, .. } => contains_lua_head(payload, head),
            Expr::Nil
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Symbol(_)
            | Expr::Local(_)
            | Expr::String(_)
            | Expr::Bytes(_) => false,
        }
}
