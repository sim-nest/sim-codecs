//! General-purpose JSON codec for the SIM runtime.
//!
//! This crate is a first-class, general-purpose codec: it round-trips every
//! kernel `Expr` losslessly by projecting the shared `Expr` graph onto
//! `serde_json::Value` using `$expr`-tagged forms (and `$located` forms when
//! source origins are carried). On top of that canonical projection it also
//! exposes interop surfaces aimed at foreign JSON consumers: a lossy untagged
//! projection mode and a small JSON Schema view for describing shapes.
//!
//! The public surface is the [`JsonCodec`] runtime object (registered via
//! [`JsonCodecLib`]) plus the free conversion functions: the canonical
//! [`expr_to_json`] / [`json_to_expr`] and located/tree variants, the
//! mode-aware projection in [`project_expr_to_json`] / [`project_json_to_expr`]
//! ([`JsonProjectionMode`]), and the schema lowering in
//! [`shape_to_json_schema`] ([`ShapeSchema`]).
//!
//! # Examples
//!
//! Register the codec and round-trip s-expression-free text through the shared
//! `Expr` graph -- decode JSON into an [`Expr`], then encode it back:
//!
//! ```
//! use std::sync::Arc;
//! use sim_codec::{Input, decode_with_codec, encode_with_codec};
//! use sim_codec_json::JsonCodecLib;
//! use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol};
//!
//! let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
//! sim_test_support::register_core_classes(&mut cx);
//!
//! let lib = JsonCodecLib::new(cx.registry_mut().fresh_codec_id());
//! cx.load_lib(&lib)?;
//! let json = Symbol::qualified("codec", "json");
//!
//! let expr = decode_with_codec(
//!     &mut cx,
//!     &json,
//!     Input::Text(r#"{"$expr":"bool","value":true}"#.to_owned()),
//!     ReadPolicy::default(),
//! )?;
//! assert_eq!(expr, Expr::Bool(true));
//!
//! let text = encode_with_codec(&mut cx, &json, &expr, Default::default())?
//!     .into_text()
//!     .unwrap();
//! assert_eq!(text, r#"{"$expr":"bool","value":true}"#);
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! The canonical projection functions can be used directly, without a runtime
//! context, for a pure `Expr <-> JSON` round-trip:
//!
//! ```
//! use sim_codec::{DecodeBudget, DecodeLimits};
//! use sim_codec_json::{expr_to_json, json_to_expr};
//! use sim_kernel::{CodecId, Expr};
//!
//! let expr = Expr::Bytes(vec![0xfb, 0xef]);
//! let value = expr_to_json(&expr);
//! assert_eq!(value["base64"], serde_json::json!("++8="));
//!
//! let mut budget = DecodeBudget::new(DecodeLimits::default());
//! let back = json_to_expr(CodecId(1), &value, &mut budget, 0)?;
//! assert_eq!(back, expr);
//! # Ok::<(), sim_kernel::Error>(())
//! ```
//!
//! [`Expr`]: sim_kernel::Expr

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod codec;
mod expr_json;
mod grammar;
mod helpers;
mod projection;
mod schema;
#[cfg(test)]
mod tests;
mod tree_json;

/// Cookbook recipes for the JSON codec, embedded at build time from the crate's
/// `recipes/` directory and exposed for help and browse surfaces.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

pub use codec::{JsonCodec, JsonCodecLib};
pub use expr_json::{expr_to_json, json_to_expr};
pub use grammar::JsonGrammarRenderer;
pub use helpers::json_escape;
pub use projection::{
    JsonProjectionMode, json_number_to_u64, project_expr_to_json, project_json_to_expr,
    project_json_to_expr_budgeted,
};
pub use schema::{ShapeSchema, shape_to_json_schema};
pub use tree_json::{json_to_located_expr, json_to_tree, located_expr_to_json, tree_to_json};
