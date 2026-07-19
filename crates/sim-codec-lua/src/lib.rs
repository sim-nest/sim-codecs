//! Lua expression lexer and parser for SIM codec surfaces.
//!
//! The crate handles the Lua source forms needed before chunk decoding:
//! lexical trivia, long strings, numeric spelling, Lua 5.4 expression
//! precedence, table constructors, indexing, calls, and method calls. It uses
//! the shared Pratt substrate for operator-table contracts while keeping
//! Lua-specific AST shapes local to this crate.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod ast;
mod lex;
mod parse_expr;
mod pratt;

pub use ast::{LuaBinOp, LuaExpr, LuaField, LuaFuncBody, LuaUnOp};
pub use lex::{LuaToken, LuaTokenKind, tokenize_lua, tokenize_lua_with_budget};
pub use parse_expr::{
    parse_lua_expr, parse_lua_expr_tree, parse_lua_expr_tree_with_budget,
    parse_lua_expr_with_budget,
};
pub use pratt::{LuaTokenSource, lua_pratt_parser, lua_pratt_table};

/// Stable local id used by direct parser helpers when no runtime codec id is
/// available yet.
pub const LUA_CODEC_ID: sim_kernel::CodecId = sim_kernel::CodecId(0x4c_55_41_00);

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
