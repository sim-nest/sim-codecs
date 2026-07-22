//! Lua chunk codec for SIM codec surfaces.
//!
//! The crate handles lexical trivia, long strings, numeric spelling, Lua 5.4
//! expression precedence, table constructors, indexing, calls, method calls,
//! statement blocks, local attributes, control-flow statements, labels, gotos,
//! and function declarations. The runtime installs `codec/lua` with plain,
//! located, and tree decode lanes plus canonical chunk encoding. Decoding lowers
//! source into ordinary `Expr` calls under the `lua` namespace so execution
//! remains a separate runtime rewrite.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod ast;
mod encode;
mod lex;
mod lower;
mod parse_block;
mod parse_expr;
mod pratt;
mod runtime;

pub use ast::{
    LuaBinOp, LuaBinding, LuaBlock, LuaExpr, LuaField, LuaFuncBody, LuaFunctionName, LuaIfArm,
    LuaLocalAttr, LuaStmt, LuaUnOp,
};
pub use encode::encode_lua_chunk_expr;
pub use lex::{LuaToken, LuaTokenKind, tokenize_lua, tokenize_lua_with_budget};
pub use lower::{
    decode_lua_chunk, decode_lua_located_chunk, decode_lua_tree_chunk, lower_lua_chunk,
};
pub use parse_block::{parse_lua_chunk, parse_lua_chunk_with_budget};
pub use parse_expr::{
    parse_lua_expr, parse_lua_expr_tree, parse_lua_expr_tree_with_budget,
    parse_lua_expr_with_budget,
};
pub use pratt::{LuaTokenSource, lua_pratt_parser, lua_pratt_table};
pub use runtime::{LuaCodec, LuaCodecLib};

/// Stable local id used by direct parser helpers when no runtime codec id is
/// available yet.
pub const LUA_CODEC_ID: sim_kernel::CodecId = sim_kernel::CodecId(0x4c_55_41_00);

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
