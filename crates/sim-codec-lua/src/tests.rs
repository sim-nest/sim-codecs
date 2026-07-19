use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{CodecId, Error, Expr, Symbol};

use crate::{
    LuaBinOp, LuaExpr, LuaField, LuaTokenKind, LuaUnOp, parse_lua_expr, parse_lua_expr_tree,
    parse_lua_expr_with_budget, tokenize_lua,
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
