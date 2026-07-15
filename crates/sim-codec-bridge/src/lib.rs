//! BRIDGE packet line codec for SIM.
//!
//! This crate provides `codec:bridge`, a strict reversible text face for one
//! BRIDGE packet per frame. It owns the packet record model, the standard part
//! and move books, content-addressed packet identity, and the ownership checks
//! used by model-facing renderers. It does not run packets, invoke tools, or
//! choose transports.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod call;
mod canonical;
mod frame_book;
mod frame_render;
mod identity;
mod line;
mod move_book;
mod ownership;
mod packet;
mod part_book;
mod profile;
mod render;
mod shape;

#[cfg(test)]
mod tests;

pub use call::{BridgeCallArgument, BridgeCallPayload, CallArgumentMedia, validate_call_payload};
pub use canonical::{BridgeCodec, BridgeCodecLib};
pub use frame_book::{
    BridgeFrameBook, BridgeFramePayload, FrameHoleKind, FrameHoleSpec, FrameKind, FrameSpec,
    standard_frame_book,
};
pub use frame_render::{render_frame, render_frame_with_prose};
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
pub use profile::{
    BridgeProfileBook, BridgeProfileSpec, ProfilePartCount, ProfilePartRule, ask_profile_spec,
    ask_profile_symbol, bridge_profile_shape_expr, brief_profile_spec, brief_profile_symbol,
    standard_profile_book,
};
pub use render::{render_frame_part, render_frame_part_with_prose};
pub use shape::bridge_packet_shape_symbol;

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
