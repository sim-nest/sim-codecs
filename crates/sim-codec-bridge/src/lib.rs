//! BRIDGE packet line codec for SIM.
//!
//! This crate provides `codec:bridge`, the strict reversible text face for one
//! BRIDGE packet per frame. BRIDGE is a single packet protocol for exchanges
//! between SIM, humans, and models; this crate owns the packet *record model* and
//! its codec, and nothing that runs, sends, or repairs a packet (that is
//! `sim-lib-bridge`).
//!
//! # The packet
//!
//! ```text
//! BridgePacket
//!   header    move, from, to, role, parents, task, output, ceiling, provenance
//!   body      an ordered list of typed parts (a closed part book):
//!             Given Frame Call Weave Check Evidence Review Vote Patch
//!             Fetch Return Receipt Attest
//!   warrant   content ids of the books/specs the packet was built against
//! ```
//!
//! BRIEF, ASK, LOOM, and COLLAB are not separate protocols -- they are
//! `BridgeProfileSpec` shapes over the body (frame-led, call-led, weave-led, and
//! review/vote/patch-led). A packet claims one profile; there is one codec and
//! one checker for all four (`standard_profile_book`).
//!
//! # What this crate guarantees
//!
//! - **Reversible identity.** `packet_content_id` / `stamp_packet_cid` /
//!   `verify_packet_cid` give a packet a content-addressed cid, and
//!   `assert_roundtrip` proves the line form (`encode_bridge_text` /
//!   `decode_bridge_text`) decodes back to the identical record.
//! - **Move and part legality.** `BridgeMoveBook` / `standard_move_book` say which
//!   move may answer which (a `vote` may not answer a `request`);
//!   `BridgePartBook` / `standard_part_book` say which typed parts are legal and
//!   whether an unknown part is rejected or preserved as inert data
//!   (`UnknownPolicy`, `AuthorityClass`).
//! - **Total text ownership.** `assert_total_ownership` proves every rendered byte
//!   a model sees belongs to a structural token, a frame sentence
//!   (`render_frame_part`), a part line, or a nonce-fenced datum -- there is no
//!   free-prose channel for a hidden instruction. A `Frame` renders both a checked
//!   record and a fluent cited sentence.
//! - **Warrants.** `warrant_for_packet` records the move/frame/part-spec content
//!   ids so a receiver can validate a packet against its *own* books across a
//!   trust or version boundary.
//!
//! The runtime side -- send/receive checking, capability ceilings, model
//! requests, the frontier engine, and the profile helpers -- lives in
//! `sim-lib-bridge`; the human review surface lives in `sim-lib-view-bridge`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod call;
mod canonical;
mod collab;
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
mod warrant;
mod weave;

#[cfg(test)]
mod tests;

pub use call::{BridgeCallArgument, BridgeCallPayload, CallArgumentMedia, validate_call_payload};
pub use canonical::{BridgeCodec, BridgeCodecLib};
pub use collab::{
    BridgeAttestPayload, BridgeEvidencePayload, BridgePatchPayload, BridgeReceiptPayload,
    BridgeReviewPayload, BridgeScore, BridgeVotePayload, validate_collab_payload,
};
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
    BridgeHeader, BridgePacket, BridgePart, BridgeProvenance, expr_to_packet, packet_to_expr,
};
pub use part_book::{
    AuthorityClass, BridgeBook, BridgePartBook, BridgePartSpec, RenderClass, UnknownPolicy,
    standard_part_book,
};
pub use profile::{
    BridgeProfileBook, BridgeProfileSpec, ProfilePartCount, ProfilePartRule, ask_profile_spec,
    ask_profile_symbol, bridge_profile_shape_expr, brief_profile_spec, brief_profile_symbol,
    collab_profile_spec, collab_profile_symbol, loom_profile_spec, loom_profile_symbol,
    standard_profile_book,
};
pub use render::{render_frame_part, render_frame_part_with_prose};
pub use shape::bridge_packet_shape_symbol;
pub use warrant::{
    BridgeWarrant, BridgeWarrantPolicy, frame_book_content_id, move_book_content_id,
    part_spec_content_id, warrant_for_packet,
};
pub use weave::{
    BridgeWeavePayload, BridgeWeaveRow, WeavePart, derive_weave_result_shape,
    validate_weave_payload,
};

/// Cookbook recipes for this codec, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
