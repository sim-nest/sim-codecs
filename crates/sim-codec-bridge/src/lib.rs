//! BRIDGE packet line codec for SIM.
//!
//! This crate provides `codec:bridge`, a strict reversible text face for one
//! BRIDGE packet per frame. It owns the packet record model, the standard part
//! and move books, content-addressed packet identity, and the ownership checks
//! used by model-facing renderers. It does not run packets, invoke tools, or
//! choose transports.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod canonical;
mod identity;
mod line;
mod move_book;
mod ownership;
mod packet;
mod part_book;
mod shape;

#[cfg(test)]
mod tests;

pub use canonical::{BridgeCodec, BridgeCodecLib};
pub use identity::{
    canonical_packet_datum, content_id_string, packet_content_id, stamp_packet_cid,
    verify_packet_cid,
};
pub use line::{decode_bridge_text, encode_bridge_text};
pub use move_book::{BridgeMoveBook, BridgeMoveSpec, ReplyRule, standard_move_book};
pub use ownership::{OwnedSpan, assert_roundtrip, assert_total_ownership};
pub use packet::{
    BridgeHeader, BridgePacket, BridgePart, BridgeProvenance, BridgeWarrant, expr_to_packet,
    packet_to_expr,
};
pub use part_book::{
    AuthorityClass, BridgeBook, BridgePartBook, BridgePartSpec, RenderClass, UnknownPolicy,
    standard_part_book,
};
pub use shape::bridge_packet_shape_symbol;

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
