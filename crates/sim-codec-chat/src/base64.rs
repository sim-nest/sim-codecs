//! Aliases for the shared standard base64 helpers used by chat byte parts.

pub(crate) use sim_codec_binary_base64::{
    decode_base64 as base64_decode, encode_base64 as base64_encode,
};
