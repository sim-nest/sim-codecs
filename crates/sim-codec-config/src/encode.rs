//! Encoding support for config text.

use sim_codec::{Encoder, Output};
use sim_kernel::{CodecId, Error, Expr, Result};

use crate::toml_lite;

/// Encoder for config maps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigEncoder;

impl ConfigEncoder {
    /// Creates a config encoder.
    pub fn new() -> Self {
        Self
    }

    /// Encodes a table or directory map without a runtime context.
    pub fn encode_text(&self, expr: &Expr) -> std::result::Result<String, String> {
        match expr {
            Expr::Map(entries) => toml_lite::encode_map(entries),
            _ => Err("config encoder expects an Expr::Map".to_owned()),
        }
    }
}

impl Encoder for ConfigEncoder {
    fn encode(&self, cx: &mut sim_kernel::WriteCx<'_>, expr: &Expr) -> Result<Output> {
        encode_expr(cx.codec, expr)
    }
}

pub(crate) fn encode_expr(codec: CodecId, expr: &Expr) -> Result<Output> {
    ConfigEncoder::new()
        .encode_text(expr)
        .map(Output::Text)
        .map_err(|message| Error::CodecError { codec, message })
}
