use std::sync::Arc;

use sim_codec::{
    DecodeBudget, Decoder, DomainCodecLib, Encoder, Input, Output, ReadCx, domain_input_text,
};
use sim_kernel::{CodecId, Error, Expr, Lib, LibManifest, Linker, LoadCx, Result, Symbol, WriteCx};

use crate::{
    BridgeBook, decode_bridge_text_with_limits, encode_bridge_text, expr_to_packet, packet_to_expr,
};

/// The `codec:bridge` decoder/encoder.
pub struct BridgeCodec;

impl Decoder for BridgeCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = domain_input_text(cx.codec, input)?;
        let budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let packet =
            decode_bridge_text_with_limits(&source, &BridgeBook::standard(), cx.codec, cx.limits)
                .map_err(|err| Error::CodecError {
                codec: cx.codec,
                message: err.to_string(),
            })?;
        Ok(packet_to_expr(&packet))
    }
}

impl Encoder for BridgeCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        let packet = expr_to_packet(expr).map_err(|err| Error::CodecError {
            codec: cx.codec,
            message: err.to_string(),
        })?;
        encode_bridge_text(&packet, &BridgeBook::standard())
            .map(Output::Text)
            .map_err(|err| Error::CodecError {
                codec: cx.codec,
                message: err.to_string(),
            })
    }
}

/// Host-registered lib that installs [`BridgeCodec`] as `codec:bridge`.
pub struct BridgeCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl BridgeCodecLib {
    /// Creates a bridge codec lib for the given codec id.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "bridge"),
            codec_id: id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(BridgeCodec),
            Arc::new(BridgeCodec),
            crate::bridge_packet_shape_symbol(),
        )
    }
}

impl Lib for BridgeCodecLib {
    fn manifest(&self) -> LibManifest {
        self.domain_lib().manifest()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        self.domain_lib().load(cx, linker)
    }
}
