//! Codec Prism contract over registered codec runtimes.
//!
//! A Prism treats each codec surface as a view over one semantic expression:
//! parse text or bytes, record spans and diagnostics, encode the same semantic
//! id at an output position, and prove whether a surface round-trips without
//! semantic loss.

use sim_kernel::{Cx, EncodeOptions, EncodePosition, Expr, ReadPolicy, SourceId, Symbol};

use crate::{Input, Output, decode_tree_with_codec, encode_with_codec};

/// A codec-aware editor contract for one codec surface.
pub trait CodecPrism {
    /// Parses text into a semantic expression id, span map, and diagnostics.
    fn parse(&self, cx: &mut Cx, text: &str) -> PrismParse;

    /// Encodes a parsed semantic id at a target output position.
    fn encode(&self, cx: &mut Cx, id: &SemanticId, position: EncodePosition) -> PrismEncode;

    /// Parses, encodes, and reparses text to prove semantic identity.
    fn round_trip(&self, cx: &mut Cx, text: &str, position: EncodePosition) -> RoundTrip;
}

/// Runtime-backed [`CodecPrism`] for an installed codec symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCodecPrism {
    codec: Symbol,
    surface: PrismSurface,
}

impl RuntimeCodecPrism {
    /// Builds a prism for a general-purpose expression codec.
    pub fn general(codec: Symbol) -> Self {
        Self {
            codec,
            surface: PrismSurface::GeneralPurpose,
        }
    }

    /// Builds a fail-closed prism for a domain codec.
    pub fn domain(codec: Symbol, domain: impl Into<String>) -> Self {
        Self {
            codec,
            surface: PrismSurface::Domain {
                name: domain.into(),
            },
        }
    }

    /// Builds a prism for the binary frame codec.
    pub fn binary(codec: Symbol) -> Self {
        Self {
            codec,
            surface: PrismSurface::BinaryInspection {
                carrier: BinaryCarrier::Bytes,
            },
        }
    }

    /// Builds a prism for the base64 text wrapper around binary frames.
    pub fn binary_base64(codec: Symbol) -> Self {
        Self {
            codec,
            surface: PrismSurface::BinaryInspection {
                carrier: BinaryCarrier::Base64Text,
            },
        }
    }

    /// The codec symbol this prism drives.
    pub fn codec(&self) -> &Symbol {
        &self.codec
    }

    /// Parses raw bytes. Text codecs receive UTF-8 validation from the codec
    /// runtime; binary codecs inspect the bytes as untrusted frame data.
    pub fn parse_bytes(&self, cx: &mut Cx, bytes: &[u8]) -> PrismParse {
        self.parse_input(cx, Input::Bytes(bytes.to_vec()), bytes.len())
    }

    /// Parses, encodes, and reparses raw bytes to prove semantic identity.
    pub fn round_trip_bytes(
        &self,
        cx: &mut Cx,
        bytes: &[u8],
        position: EncodePosition,
    ) -> RoundTrip {
        self.round_trip_input(cx, Input::Bytes(bytes.to_vec()), bytes.len(), position)
    }

    fn parse_input(&self, cx: &mut Cx, input: Input, source_len: usize) -> PrismParse {
        let input_kind = match &input {
            Input::Text(_) => PrismInputKind::Text,
            Input::Bytes(_) => PrismInputKind::Bytes,
        };
        let source_id = format!("codec-prism:{}", self.codec);
        match decode_tree_with_codec(
            cx,
            &self.codec,
            input.clone(),
            ReadPolicy::default(),
            source_id.clone(),
        ) {
            Ok(tree) => {
                let semantic_id = SemanticId::from_expr(tree.expr.clone());
                let mut span_map = Vec::new();
                collect_spans(&tree, &mut span_map);
                if span_map.is_empty() {
                    span_map.push(PrismSpan {
                        source: SourceId(source_id),
                        start: 0,
                        end: source_len,
                    });
                }
                let diagnostics = self.surface_diagnostics(true, None);
                PrismParse {
                    codec: self.codec.clone(),
                    semantic_id: Some(semantic_id),
                    expr: Some(tree.expr),
                    span_map,
                    diagnostics,
                    inspection: PrismInspection::new(input_kind, self.surface.is_executable()),
                }
            }
            Err(error) => PrismParse {
                codec: self.codec.clone(),
                semantic_id: None,
                expr: None,
                span_map: Vec::new(),
                diagnostics: self.surface_diagnostics(false, Some(error.to_string())),
                inspection: PrismInspection::new(input_kind, self.surface.is_executable()),
            },
        }
    }

    fn surface_diagnostics(&self, accepted: bool, error: Option<String>) -> Vec<PrismDiagnostic> {
        match (&self.surface, accepted, error) {
            (PrismSurface::Domain { name }, false, Some(error)) => vec![PrismDiagnostic::error(
                "domain-rejected",
                format!("{name} codec rejected non-domain input: {error}"),
            )],
            (_, false, Some(error)) => {
                vec![PrismDiagnostic::error("parse-error", error)]
            }
            _ => Vec::new(),
        }
    }

    fn output_to_input(&self, output: &PrismOutput) -> Input {
        match output {
            PrismOutput::Text(text) => Input::Text(text.clone()),
            PrismOutput::Bytes(bytes) => Input::Bytes(bytes.clone()),
        }
    }

    fn round_trip_input(
        &self,
        cx: &mut Cx,
        input: Input,
        source_len: usize,
        position: EncodePosition,
    ) -> RoundTrip {
        let parse = self.parse_input(cx, input, source_len);
        let encode = parse
            .semantic_id
            .as_ref()
            .map(|id| self.encode(cx, id, position))
            .unwrap_or_else(|| PrismEncode {
                codec: self.codec.clone(),
                position,
                output: None,
                diagnostics: vec![PrismDiagnostic::error(
                    "parse-missing",
                    "parse did not produce a semantic id",
                )],
            });
        let reparsed = encode.output.as_ref().map(|output| {
            let input = self.output_to_input(output);
            let len = output.len();
            self.parse_input(cx, input, len)
        });
        let loss_report = LossReport::from_parts(&parse, &encode, reparsed.as_ref());
        RoundTrip {
            parse,
            encode,
            reparsed,
            loss_report,
        }
    }
}

impl CodecPrism for RuntimeCodecPrism {
    fn parse(&self, cx: &mut Cx, text: &str) -> PrismParse {
        self.parse_input(cx, Input::Text(text.to_owned()), text.len())
    }

    fn encode(&self, cx: &mut Cx, id: &SemanticId, position: EncodePosition) -> PrismEncode {
        let Some(expr) = &id.expr else {
            return PrismEncode {
                codec: self.codec.clone(),
                position,
                output: None,
                diagnostics: vec![PrismDiagnostic::error(
                    "semantic-id-missing",
                    "semantic id does not carry an expression for encoding",
                )],
            };
        };
        let options = EncodeOptions {
            position,
            ..EncodeOptions::default()
        };
        match encode_with_codec(cx, &self.codec, expr, options) {
            Ok(Output::Text(text)) => PrismEncode {
                codec: self.codec.clone(),
                position,
                output: Some(PrismOutput::Text(text)),
                diagnostics: Vec::new(),
            },
            Ok(Output::Bytes(bytes)) => PrismEncode {
                codec: self.codec.clone(),
                position,
                output: Some(PrismOutput::Bytes(bytes)),
                diagnostics: Vec::new(),
            },
            Err(error) => PrismEncode {
                codec: self.codec.clone(),
                position,
                output: None,
                diagnostics: vec![PrismDiagnostic::error("encode-error", error.to_string())],
            },
        }
    }

    fn round_trip(&self, cx: &mut Cx, text: &str, position: EncodePosition) -> RoundTrip {
        self.round_trip_input(cx, Input::Text(text.to_owned()), text.len(), position)
    }
}

/// The class of codec surface a Prism is driving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrismSurface {
    /// General-purpose expression codec.
    GeneralPurpose,
    /// Domain codec that fails closed outside `name`.
    Domain {
        /// The domain label shown in diagnostics.
        name: String,
    },
    /// Binary frame inspection surface.
    BinaryInspection {
        /// How the bytes are carried.
        carrier: BinaryCarrier,
    },
}

impl PrismSurface {
    fn is_executable(&self) -> bool {
        false
    }
}

/// How binary frame bytes are carried at the codec boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryCarrier {
    /// Raw bytes.
    Bytes,
    /// Base64 text.
    Base64Text,
}

/// What kind of input the Prism inspected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrismInputKind {
    /// UTF-8 text input.
    Text,
    /// Raw byte input.
    Bytes,
}

/// Metadata describing how input was inspected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrismInspection {
    /// Input carrier type.
    pub input: PrismInputKind,
    /// Whether the Prism treats the input as trusted executable code.
    pub trusted_executable: bool,
}

impl PrismInspection {
    fn new(input: PrismInputKind, trusted_executable: bool) -> Self {
        Self {
            input,
            trusted_executable,
        }
    }
}

/// Stable identity for a semantic expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticId {
    /// Stable display id for comparing Prism results.
    pub stable: String,
    /// The expression behind the id, retained for immediate re-encoding.
    pub expr: Option<Expr>,
}

impl SemanticId {
    /// Builds a semantic id from an expression's canonical key.
    pub fn from_expr(expr: Expr) -> Self {
        let stable = format!(
            "expr:{}",
            stable_hash(&format!("{:?}", expr.canonical_key()))
        );
        Self {
            stable,
            expr: Some(expr),
        }
    }
}

/// A half-open byte span belonging to a parsed surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrismSpan {
    /// Source id.
    pub source: SourceId,
    /// Inclusive start byte.
    pub start: usize,
    /// Exclusive end byte.
    pub end: usize,
}

/// A parse diagnostic surfaced by the Prism.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrismDiagnostic {
    /// Severity label.
    pub severity: DiagnosticSeverity,
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Optional source span.
    pub span: Option<PrismSpan>,
}

impl PrismDiagnostic {
    /// Creates an error diagnostic without a span.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code: code.into(),
            message: message.into(),
            span: None,
        }
    }
}

/// Diagnostic severity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Informational diagnostic.
    Info,
    /// Warning diagnostic.
    Warning,
    /// Error diagnostic.
    Error,
}

/// Parse result for one codec surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrismParse {
    /// Codec symbol used for parsing.
    pub codec: Symbol,
    /// Semantic id, if parsing succeeded.
    pub semantic_id: Option<SemanticId>,
    /// Parsed expression, if parsing succeeded.
    pub expr: Option<Expr>,
    /// Span map over the parsed input.
    pub span_map: Vec<PrismSpan>,
    /// Parse diagnostics.
    pub diagnostics: Vec<PrismDiagnostic>,
    /// Inspection metadata.
    pub inspection: PrismInspection,
}

/// Output from a Prism encode pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrismOutput {
    /// Text output.
    Text(String),
    /// Raw byte output.
    Bytes(Vec<u8>),
}

impl PrismOutput {
    /// Display-safe representation of the output.
    pub fn display(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Bytes(bytes) => {
                let hex = bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join("");
                format!("{} bytes: {hex}", bytes.len())
            }
        }
    }

    /// Output length in its carrier units.
    pub fn len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Bytes(bytes) => bytes.len(),
        }
    }

    /// Whether the output is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Encode result for one codec surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrismEncode {
    /// Codec symbol used for encoding.
    pub codec: Symbol,
    /// Target output position.
    pub position: EncodePosition,
    /// Encoded output, if encoding succeeded.
    pub output: Option<PrismOutput>,
    /// Encode diagnostics.
    pub diagnostics: Vec<PrismDiagnostic>,
}

/// Loss report for one parse/encode/reparse cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LossReport {
    /// Whether the whole cycle had no diagnostics and preserved semantic id.
    pub lossless: bool,
    /// Whether parse and reparse produced the same semantic identity.
    pub semantic_identity: bool,
    /// Diagnostics collected across the cycle.
    pub diagnostics: Vec<PrismDiagnostic>,
}

impl LossReport {
    fn from_parts(parse: &PrismParse, encode: &PrismEncode, reparsed: Option<&PrismParse>) -> Self {
        let semantic_identity = match (
            parse.semantic_id.as_ref(),
            reparsed.and_then(|parse| parse.semantic_id.as_ref()),
        ) {
            (Some(left), Some(right)) => left.stable == right.stable,
            _ => false,
        };
        let mut diagnostics = Vec::new();
        diagnostics.extend(parse.diagnostics.clone());
        diagnostics.extend(encode.diagnostics.clone());
        if let Some(reparsed) = reparsed {
            diagnostics.extend(reparsed.diagnostics.clone());
        }
        if !semantic_identity {
            diagnostics.push(PrismDiagnostic::error(
                "semantic-identity-loss",
                "parse and reparse semantic ids differ",
            ));
        }
        Self {
            lossless: semantic_identity && diagnostics.is_empty(),
            semantic_identity,
            diagnostics,
        }
    }
}

/// Full round-trip proof for one codec surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundTrip {
    /// Initial parse result.
    pub parse: PrismParse,
    /// Encode result.
    pub encode: PrismEncode,
    /// Parse result for the encoded output.
    pub reparsed: Option<PrismParse>,
    /// Loss report for the cycle.
    pub loss_report: LossReport,
}

fn collect_spans(tree: &sim_kernel::LocatedExprTree, spans: &mut Vec<PrismSpan>) {
    if let Some(origin) = &tree.origin {
        spans.push(PrismSpan {
            source: origin.source.clone(),
            start: origin.span.start,
            end: origin.span.end,
        });
    }
    for child in &tree.children {
        collect_spans(child, spans);
    }
}

fn stable_hash(text: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
