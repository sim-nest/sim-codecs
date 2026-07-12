//! Implementation root for sim-codec.
//!
//! Declares and aggregates the runtime (decoder/encoder), domain, domain-form,
//! limits, list/expr encode, lowering, portable, strings, and tree submodules,
//! and re-exports their public surface to the crate root.

#![forbid(unsafe_code)]

mod domain;
mod domain_form;
mod limits;
mod list_encode;
mod lowering;
mod portable;
mod runtime;
mod strings;
mod tree;

pub use domain::{DomainCodecLib, domain_input_text, resolve_expr_shape, resolve_options_shape};
pub use domain_form::{
    DomainForm, DomainFormError, DomainValue, format_domain_form, parse_domain_form,
};
pub use limits::{DecodeBudget, DecodeLimits, ReadCx};
pub use list_encode::{encode_value_expr, force_list_for_encode};
pub use lowering::lower_operator_nodes;
pub use portable::{decode_portable, encode_portable};
pub use runtime::{
    CodecDefaultDecode, CodecRuntime, DecodePosition, DecodeTarget, Decoder, Encoder, Input,
    LocatedDecoder, LocatedEncoder, Output, TreeDecoder, TreeEncoder,
};
pub use strings::{decode_string_literal, encode_string_literal};
pub use tree::validate_expr_tree;
