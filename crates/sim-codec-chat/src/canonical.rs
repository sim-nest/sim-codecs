//! The `codec:chat` decoder/encoder pair and its host-registered lib. Decodes
//! the canonical `SIMCHAT1` transcript text into checked `Expr` and encodes
//! checked transcripts back out, validating the transcript shape on both sides.

use std::sync::Arc;

use sim_codec::{
    DecodeBudget, Decoder, DomainCodecLib, Encoder, Input, Output, ReadCx, domain_input_text,
};
use sim_kernel::{CodecId, Export, Lib, LibManifest, Linker, LoadCx, Result, Symbol, WriteCx};

use crate::cookbook::{
    ChatProviderProfilesReport, ChatTranscriptRoundtripReport, provider_profiles_symbol,
    transcript_roundtrip_symbol,
};
use crate::{
    expr::{decode_chat_text, encode_chat_text},
    validate_chat_transcript,
};

/// Provider-neutral transcript codec.
pub struct ChatCodec;

impl Decoder for ChatCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr> {
        let source = domain_input_text(cx.codec, input)?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let expr = decode_chat_text(cx.codec, &source, &mut budget)?;
        validate_chat_transcript(&expr)?;
        Ok(expr)
    }
}

impl Encoder for ChatCodec {
    fn encode(&self, _cx: &mut WriteCx<'_>, expr: &sim_kernel::Expr) -> Result<Output> {
        validate_chat_transcript(expr)?;
        Ok(Output::Text(encode_chat_text(expr)))
    }
}

/// Host-registered lib for `codec:chat`, built on the shared
/// [`DomainCodecLib`] scaffold.
pub struct ChatCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl ChatCodecLib {
    /// Creates the lib bound to the given runtime-assigned codec id.
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "chat"),
            codec_id: id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(ChatCodec),
            Arc::new(ChatCodec),
            Symbol::qualified("codec", "ChatTranscript"),
        )
    }
}

impl Lib for ChatCodecLib {
    fn manifest(&self) -> LibManifest {
        let mut manifest = self.domain_lib().manifest();
        manifest.exports.extend([
            Export::Function {
                symbol: transcript_roundtrip_symbol(),
                function_id: None,
            },
            Export::Function {
                symbol: provider_profiles_symbol(),
                function_id: None,
            },
        ]);
        manifest
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        self.domain_lib().load(cx, linker)?;
        linker.function_value(
            transcript_roundtrip_symbol(),
            cx.factory()
                .opaque(Arc::new(ChatTranscriptRoundtripReport))?,
        )?;
        linker.function_value(
            provider_profiles_symbol(),
            cx.factory().opaque(Arc::new(ChatProviderProfilesReport))?,
        )?;
        Ok(())
    }
}
