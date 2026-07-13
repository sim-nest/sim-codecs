//! Configuration codec for the SIM runtime.
//!
//! The crate reads small ASCII configuration files into [`Expr::Map`] values and
//! writes map values back to canonical config text. A per-library file decodes
//! as one table; a shared `sim.toml`-style file decodes as a directory mapping
//! library ids to tables. The runtime codec symbol is `codec/config`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::sync::Arc;

use sim_codec::{CodecDefaultDecode, CodecRuntime, Decoder, Encoder, Input, codec_value};
use sim_kernel::{
    AbiVersion, CodecId, Dependency, Export, Expr, Lib, LibManifest, LibTarget, Linker, Result,
    Symbol, Version,
};

mod decode;
mod encode;
mod toml_lite;

#[cfg(test)]
mod tests;

pub use decode::{ConfigDecoder, DecodeMode};
pub use encode::ConfigEncoder;

/// Embedded cookbook recipes for the config codec.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

/// Runtime decoder/encoder for the `codec/config` surface.
///
/// Decode uses a small shape heuristic: inputs with only library-id sections
/// decode as config directories, while per-library files decode as one table.
/// Use [`ConfigDecoder`] directly when the caller needs a fixed mode.
pub struct ConfigCodec;

impl Decoder for ConfigCodec {
    fn decode(&self, cx: &mut sim_codec::ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input.into_string()?;
        let budget = sim_codec::DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        decode::decode_auto_text(cx.codec, &source)
    }
}

impl Encoder for ConfigCodec {
    fn encode(&self, cx: &mut sim_kernel::WriteCx<'_>, expr: &Expr) -> Result<sim_codec::Output> {
        encode::encode_expr(cx.codec, expr)
    }
}

/// Host-registered library that installs `codec/config`.
pub struct ConfigCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl ConfigCodecLib {
    /// Creates the codec lib bound to the runtime-assigned id for
    /// `codec/config`.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "config"),
            codec_id: id,
        }
    }
}

impl Lib for ConfigCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol: self.symbol.clone(),
                codec_id: Some(self.codec_id),
            }],
        }
    }

    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker) -> Result<()> {
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "Config"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;
        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(ConfigCodec)),
                located_decoder: None,
                tree_decoder: None,
                encoder: Some(Arc::new(ConfigCodec)),
                located_encoder: None,
                tree_encoder: None,
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        Ok(())
    }
}
