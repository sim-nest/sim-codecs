use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{Args, Callable, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value};
use sim_value::build::entry as field;

use crate::expr::{decode_chat_text, encode_chat_text};
use crate::{
    CodecProfile, RequestWire, StreamWire, anthropic_profile, lemonade_profile, lm_studio_profile,
    model_response_expr, ollama_profile, openai_profile, text_part, validate_chat_transcript,
};

pub(crate) fn transcript_roundtrip_symbol() -> Symbol {
    Symbol::qualified("chat", "transcript-roundtrip")
}

pub(crate) fn provider_profiles_symbol() -> Symbol {
    Symbol::qualified("chat", "provider-profiles")
}

pub(crate) struct ChatTranscriptRoundtripReport;

impl Callable for ChatTranscriptRoundtripReport {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        if !args.values().is_empty() {
            return Err(Error::Eval(format!(
                "{} expects no arguments",
                transcript_roundtrip_symbol()
            )));
        }
        let sample = model_response_expr(
            Symbol::new("cookbook"),
            "codec-demo",
            vec![text_part("The chat codec round-trips this transcript.")],
            Symbol::new("stop"),
        );
        validate_chat_transcript(&sample)?;
        let text = encode_chat_text(&sample);
        let mut budget = DecodeBudget::new(DecodeLimits::default());
        let decoded = decode_chat_text(sim_kernel::CodecId(0), &text, &mut budget)?;
        let report = Expr::Map(vec![
            field(
                "kind",
                Expr::Symbol(Symbol::qualified("codec", "roundtrip")),
            ),
            field("codec", Expr::String("codec/chat".to_owned())),
            field("wire", Expr::String("SIMCHAT1".to_owned())),
            field("encoded-chars", Expr::String(text.len().to_string())),
            field("decoded", decoded.clone()),
            field("roundtrip", Expr::Bool(decoded.canonical_eq(&sample))),
            field(
                "lanes",
                Expr::List(vec![
                    Expr::String("encode".to_owned()),
                    Expr::String("decode".to_owned()),
                ]),
            ),
        ]);
        cx.factory().expr(report)
    }
}

pub(crate) struct ChatProviderProfilesReport;

impl Callable for ChatProviderProfilesReport {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        if !args.values().is_empty() {
            return Err(Error::Eval(format!(
                "{} expects no arguments",
                provider_profiles_symbol()
            )));
        }
        let profiles = [
            openai_profile(),
            anthropic_profile(),
            ollama_profile(),
            lm_studio_profile(),
            lemonade_profile(),
        ];
        cx.factory().expr(Expr::Map(vec![
            field("kind", Expr::Symbol(Symbol::qualified("chat", "profiles"))),
            field(
                "providers",
                Expr::List(profiles.iter().map(profile_expr).collect()),
            ),
            field("count", Expr::String(profiles.len().to_string())),
            field("mode", Expr::String("modeled".to_owned())),
        ]))
    }
}

impl Object for ChatTranscriptRoundtripReport {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(transcript_roundtrip_symbol().to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ChatTranscriptRoundtripReport {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Object for ChatProviderProfilesReport {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(provider_profiles_symbol().to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ChatProviderProfilesReport {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

fn profile_expr(profile: &CodecProfile) -> Expr {
    Expr::Map(vec![
        field("provider", Expr::String(profile.provider.to_string())),
        field("codec", Expr::String(profile.codec.to_string())),
        field(
            "request-wire",
            Expr::String(request_wire_name(profile.request_wire).to_owned()),
        ),
        field(
            "stream-wire",
            Expr::String(stream_wire_name(profile.stream_wire).to_owned()),
        ),
    ])
}

fn request_wire_name(wire: RequestWire) -> &'static str {
    match wire {
        RequestWire::OpenAiResponses => "openai-responses",
        RequestWire::OpenAiChat => "openai-chat",
        RequestWire::AnthropicMessages => "anthropic-messages",
        RequestWire::OllamaChat => "ollama-chat",
    }
}

fn stream_wire_name(wire: StreamWire) -> &'static str {
    match wire {
        StreamWire::None => "none",
        StreamWire::Sse => "sse",
        StreamWire::Ndjson => "ndjson",
    }
}
