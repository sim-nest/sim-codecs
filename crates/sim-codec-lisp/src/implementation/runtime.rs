//! Runtime wiring for the Lisp codec: the `Lib` implementation that builds the
//! manifest and registers the codec's decoder and encoder with the linker.

use std::sync::Arc;

use sim_codec::{CodecDefaultDecode, CodecRuntime, codec_value};
use sim_kernel::{
    AbiVersion, DefaultFactory, Dependency, Export, Lib, LibManifest, LibTarget, Linker, Result,
    Symbol, Version,
};

use super::{
    LispProcMacroDecoder, LispProcMacroEncoder,
    cli::{LispCliEntrypoint, cli_main_symbol},
};

/// [`Lib`] that registers the Lisp codec with the runtime.
///
/// Its manifest exports the `codec/lisp` codec, and loading wires the
/// [`LispProcMacroDecoder`] and [`LispProcMacroEncoder`] into the linker as the
/// codec's decode and encode surfaces.
pub struct LispCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}

impl LispCodecLib {
    /// Creates the codec lib bound to the runtime-assigned `id` for `codec/lisp`.
    pub fn new(id: sim_kernel::CodecId) -> Result<Self> {
        Ok(Self {
            symbol: Symbol::qualified("codec", "lisp"),
            codec_id: id,
        })
    }
}

impl Lib for LispCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: vec![
                Export::Codec {
                    symbol: self.symbol.clone(),
                    codec_id: Some(self.codec_id),
                },
                Export::Function {
                    symbol: cli_main_symbol(),
                    function_id: None,
                },
            ],
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker) -> Result<()> {
        let _factory = DefaultFactory;
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "LispSurface"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;
        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(LispProcMacroDecoder)),
                located_decoder: Some(Arc::new(LispProcMacroDecoder)),
                tree_decoder: Some(Arc::new(LispProcMacroDecoder)),
                encoder: Some(Arc::new(LispProcMacroEncoder)),
                located_encoder: None,
                tree_encoder: Some(Arc::new(LispProcMacroEncoder)),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::TermInEvalDatumOtherwise,
            }),
        )?;
        linker.function_value(
            cli_main_symbol(),
            cx.factory().opaque(Arc::new(LispCliEntrypoint))?,
        )?;
        Ok(())
    }
}
