//! Complete dated, transport-independent MCP wire authority.

mod helpers;

use helpers::*;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_codec_json::SchemaDocument;

const MODERN: &str = "2026-07-28";
const LEGACY: &str = "2025-03-26";
const MAX_EXTENSION_BYTES: usize = 64 * 1024;
const MAX_EXTENSION_DEPTH: usize = 16;

/// The two exact protocol profiles accepted by this codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolProfileId {
    /// Final stateless protocol.
    Modern20260728,
    /// Delivered compatibility protocol.
    Legacy20250326,
}

impl ProtocolProfileId {
    /// Parse an exact wire revision or return requested and supported revisions.
    pub fn parse(requested: &str) -> Result<Self, ProtocolError> {
        match requested {
            MODERN => Ok(Self::Modern20260728),
            LEGACY => Ok(Self::Legacy20250326),
            _ => Err(ProtocolError::UnsupportedVersion {
                requested: requested.to_owned(),
                supported: [MODERN, LEGACY],
            }),
        }
    }

    /// Exact dated wire spelling.
    pub const fn revision(self) -> &'static str {
        match self {
            Self::Modern20260728 => MODERN,
            Self::Legacy20250326 => LEGACY,
        }
    }
}

/// Protocol negotiation failure with exact diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    /// The requested revision is not implemented.
    UnsupportedVersion {
        /// Exact requested value.
        requested: String,
        /// Exact supported values, newest first.
        supported: [&'static str; 2],
    },
}

/// Bounded, semantic JSON settings owned by negotiated extensions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionSettings(BTreeMap<String, Value>);

impl ExtensionSettings {
    /// Admit namespaced settings under the codec's byte and depth budgets.
    pub fn new(values: BTreeMap<String, Value>) -> Result<Self, WireError> {
        for (key, value) in &values {
            extension_owner(key).ok_or_else(|| WireError::ExtensionCollision(key.clone()))?;
            if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > MAX_EXTENSION_BYTES)
                || json_depth(value) > MAX_EXTENSION_DEPTH
            {
                return Err(WireError::ExtensionLimit(key.clone()));
            }
        }
        Ok(Self(values))
    }

    /// Exact semantic JSON settings.
    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.0
    }

    /// Verify every behavior-bearing setting belongs to a negotiated owner.
    pub fn validate_negotiated(&self, owners: &BTreeSet<String>) -> Result<(), WireError> {
        for key in self.0.keys() {
            let owner =
                extension_owner(key).ok_or_else(|| WireError::ExtensionCollision(key.clone()))?;
            if !owners.contains(owner) {
                return Err(WireError::UnnegotiatedExtension(owner.to_owned()));
            }
        }
        Ok(())
    }
}

impl Serialize for ExtensionSettings {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExtensionSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = BTreeMap::<String, Value>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

/// W3C trace fields carried in MCP metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceContext {
    /// W3C trace-parent value.
    pub traceparent: Option<String>,
    /// W3C trace-state value.
    pub tracestate: Option<String>,
    /// W3C baggage value.
    pub baggage: Option<String>,
}

impl TraceContext {
    /// Reject malformed or dependent tracing fields.
    pub fn validate(&self) -> Result<(), WireError> {
        if self.tracestate.is_some() && self.traceparent.is_none() {
            return Err(WireError::MalformedTrace("tracestate requires traceparent"));
        }
        if let Some(value) = &self.traceparent {
            let parts: Vec<_> = value.split('-').collect();
            if parts.len() != 4
                || parts[0].len() != 2
                || parts[1].len() != 32
                || parts[2].len() != 16
                || parts[3].len() != 2
                || !parts
                    .iter()
                    .all(|part| part.bytes().all(|b| b.is_ascii_hexdigit()))
                || parts[1].bytes().all(|b| b == b'0')
                || parts[2].bytes().all(|b| b == b'0')
            {
                return Err(WireError::MalformedTrace("invalid traceparent"));
            }
        }
        Ok(())
    }
}

/// Named and versioned MCP implementation metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Display name.
    pub name: String,
    /// Implementation version.
    pub version: String,
}

/// Typed client metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientMeta {
    /// Advertised capabilities.
    pub capabilities: Value,
    /// Client identity.
    pub info: Option<ClientInfo>,
    /// Negotiated extension settings.
    #[serde(flatten)]
    pub extensions: ExtensionSettings,
}

/// Complete typed request `_meta`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestMeta {
    /// Exact protocol version.
    pub protocol_version: String,
    /// Client capability document.
    pub client_capabilities: Value,
    /// Optional client identity.
    pub client_info: Option<ClientInfo>,
    /// Trace context.
    #[serde(flatten)]
    pub trace: TraceContext,
    /// Negotiated extension settings.
    #[serde(flatten)]
    pub extensions: ExtensionSettings,
}

/// Server identity metadata. It is descriptive and never security authority.
pub type ServerInfo = ClientInfo;

/// Typed server metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerMeta {
    /// Server capabilities.
    pub capabilities: Value,
    /// Descriptive server identity.
    pub server_info: Option<ServerInfo>,
    /// Trace context.
    #[serde(flatten)]
    pub trace: TraceContext,
    /// Negotiated extension settings.
    #[serde(flatten)]
    pub extensions: ExtensionSettings,
}

/// Result of `server/discover`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    /// Supported versions, in preference order.
    pub supported_versions: Vec<String>,
    /// Server capabilities.
    pub server_capabilities: Value,
    /// Descriptive server identity.
    pub server_info: ServerInfo,
}

/// Caller answers supplied after an input-required result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputResponses {
    /// Named semantic JSON answers.
    pub input_responses: BTreeMap<String, Value>,
    /// Exact state previously returned by the server.
    pub request_state: Value,
}

/// `subscriptions/listen` request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListen {
    /// Subscription identifier.
    pub subscription_id: String,
}

/// Delivery acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Acknowledgement {
    /// Subscription identifier.
    pub subscription_id: String,
    /// Monotonic event sequence.
    pub sequence: u64,
}

/// Subscription event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventMessage {
    /// Subscription identifier.
    pub subscription_id: String,
    /// Monotonic event sequence.
    pub sequence: u64,
    /// Semantic event body.
    pub event: Value,
}

/// Cancellation message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelMessage {
    /// Request identifier being cancelled.
    pub request_id: Value,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

/// Terminal subscription message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalMessage {
    /// Subscription identifier.
    pub subscription_id: String,
    /// Last sequence in the stream.
    pub sequence: u64,
    /// Optional final error.
    pub error: Option<FinalError>,
}

/// Methods whose wire requirements are centralized in [`method_registry`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    /// Discover versions and capabilities.
    ServerDiscover,
    /// List tools.
    ToolsList,
    /// Call a tool.
    ToolsCall,
    /// List prompts.
    PromptsList,
    /// Fetch a prompt.
    PromptsGet,
    /// List resources.
    ResourcesList,
    /// Read a resource.
    ResourcesRead,
    /// Begin a subscription.
    SubscriptionsListen,
    /// Retry an input-required result with input responses.
    InputResponses,
    /// Acknowledge a delivered event.
    Acknowledgement,
    /// Deliver a subscription event.
    Event,
    /// Cancel work.
    Cancel,
    /// Mark a stream terminal.
    Terminal,
}

/// Generated server/client policy for one method.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MethodRule {
    /// Wire name.
    pub name: &'static str,
    /// Whether an HTTP method projection is required.
    pub requires_method_header: bool,
    /// Whether an MCP name projection is required.
    pub requires_name_header: bool,
    /// Whether complete results may be cached.
    pub cache_eligible: bool,
}

/// Single method registry consumed by both endpoints.
pub const fn method_registry(method: Method) -> MethodRule {
    match method {
        Method::ServerDiscover => MethodRule {
            name: "server/discover",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: true,
        },
        Method::ToolsList => MethodRule {
            name: "tools/list",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: true,
        },
        Method::ToolsCall => MethodRule {
            name: "tools/call",
            requires_method_header: true,
            requires_name_header: true,
            cache_eligible: false,
        },
        Method::PromptsList => MethodRule {
            name: "prompts/list",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: true,
        },
        Method::PromptsGet => MethodRule {
            name: "prompts/get",
            requires_method_header: true,
            requires_name_header: true,
            cache_eligible: true,
        },
        Method::ResourcesList => MethodRule {
            name: "resources/list",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: true,
        },
        Method::ResourcesRead => MethodRule {
            name: "resources/read",
            requires_method_header: true,
            requires_name_header: true,
            cache_eligible: true,
        },
        Method::SubscriptionsListen => MethodRule {
            name: "subscriptions/listen",
            requires_method_header: true,
            requires_name_header: true,
            cache_eligible: false,
        },
        Method::InputResponses => MethodRule {
            name: "inputResponses",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: false,
        },
        Method::Acknowledgement => MethodRule {
            name: "acknowledgement",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: false,
        },
        Method::Event => MethodRule {
            name: "event",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: false,
        },
        Method::Cancel => MethodRule {
            name: "cancel",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: false,
        },
        Method::Terminal => MethodRule {
            name: "terminal",
            requires_method_header: true,
            requires_name_header: false,
            cache_eligible: false,
        },
    }
}

/// Negotiated result discriminator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResultType {
    /// Finished result.
    Complete,
    /// Caller input is required.
    InputRequired,
    /// Namespaced, negotiated extension result.
    Extension(String),
}

impl ResultType {
    /// Interpret a profile-specific result token. Legacy omission normalizes to complete.
    pub fn for_profile(
        profile: ProtocolProfileId,
        token: Option<&str>,
        negotiated: &BTreeSet<String>,
    ) -> Result<Self, WireError> {
        match (profile, token) {
            (ProtocolProfileId::Legacy20250326, None) => Ok(Self::Complete),
            (ProtocolProfileId::Modern20260728, None) => {
                Err(WireError::MissingProjection("resultType"))
            }
            (_, Some(token)) => Self::parse(token, negotiated),
        }
    }

    /// Parse and validate a result token against negotiated extension owners.
    pub fn parse(token: &str, negotiated: &BTreeSet<String>) -> Result<Self, WireError> {
        match token {
            "complete" => Ok(Self::Complete),
            "input_required" => Ok(Self::InputRequired),
            other => {
                let owner = extension_owner(other)
                    .ok_or_else(|| WireError::InvalidResultType(other.to_owned()))?;
                if negotiated.contains(owner) {
                    Ok(Self::Extension(other.to_owned()))
                } else {
                    Err(WireError::UnnegotiatedExtension(owner.to_owned()))
                }
            }
        }
    }
}

/// Input-required payload.
#[derive(Clone, Debug, PartialEq)]
pub struct InputRequired {
    /// Named JSON Schema request documents.
    pub input_requests: BTreeMap<String, SchemaDocument>,
    /// Opaque round-trip state.
    pub request_state: Value,
}

/// Payload associated with a result discriminator.
#[derive(Clone, Debug, PartialEq)]
pub enum ResultPayload {
    /// Complete ordinary content.
    Complete(Value),
    /// Required input and state.
    InputRequired(InputRequired),
    /// Exact extension payload.
    Extension(Value),
}

/// Cache policy supplied by a complete result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheHint {
    /// Explicit eligibility.
    pub cacheable: bool,
    /// Maximum freshness in seconds.
    pub max_age: Option<u64>,
}

/// Modern result, constructible only through validation that attaches server info.
#[derive(Clone, Debug, PartialEq)]
pub struct ModernResult {
    /// Validated discriminator.
    pub result_type: ResultType,
    /// Matching payload.
    pub payload: ResultPayload,
    /// Uniform descriptive server identity.
    pub server_info: ServerInfo,
    /// Optional validated cache hint.
    pub cache: Option<CacheHint>,
}

impl ModernResult {
    /// Construct a modern result and enforce type/payload/cache consistency.
    pub fn new(
        method: Method,
        result_type: ResultType,
        payload: ResultPayload,
        server_info: ServerInfo,
        cache: Option<CacheHint>,
    ) -> Result<Self, WireError> {
        let matches = matches!(
            (&result_type, &payload),
            (ResultType::Complete, ResultPayload::Complete(_))
                | (ResultType::InputRequired, ResultPayload::InputRequired(_))
                | (ResultType::Extension(_), ResultPayload::Extension(_))
        );
        if !matches {
            return Err(WireError::InvalidStatusBody);
        }
        if cache.is_some_and(|hint| hint.cacheable)
            && (!method_registry(method).cache_eligible
                || !matches!(result_type, ResultType::Complete))
        {
            return Err(WireError::InvalidStatusBody);
        }
        Ok(Self {
            result_type,
            payload,
            server_info,
            cache,
        })
    }
}

/// Full JSON Schema documents for tool input, output, and structured content.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSchemas {
    /// Tool input contract.
    pub input: SchemaDocument,
    /// Optional tool output contract.
    pub output: Option<SchemaDocument>,
    /// Optional structured-content contract.
    pub structured_content: Option<SchemaDocument>,
}

/// Final protocol error record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinalError {
    /// Stable numeric code.
    pub code: i64,
    /// Stable protocol name.
    pub name: String,
    /// Human-readable message.
    pub message: String,
    /// Exact structured details.
    pub data: Value,
}

/// Pure HTTP header projection derived from a decoded body.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeaderProjection {
    /// Exact protocol revision.
    pub protocol: Option<String>,
    /// Projected method.
    pub method: Option<String>,
    /// Projected addressed name.
    pub name: Option<String>,
    /// Safe-value or `:base64:` sentinel headers.
    pub x_mcp_headers: BTreeMap<String, String>,
}

impl HeaderProjection {
    /// Derive the sole authoritative projection from validated body fields.
    pub fn from_body(
        profile: ProtocolProfileId,
        method: Method,
        name: Option<&str>,
        meta: &RequestMeta,
    ) -> Result<Self, WireError> {
        meta.trace.validate()?;
        if meta.protocol_version != profile.revision() {
            return Err(WireError::InvalidStatusBody);
        }
        let rule = method_registry(method);
        if rule.requires_name_header && name.is_none() {
            return Err(WireError::MissingProjection("name"));
        }
        let mut custom = BTreeMap::new();
        custom.insert(
            "MCP-Client-Capabilities".to_owned(),
            encode_header_json(&meta.client_capabilities),
        );
        if let Some(info) = &meta.client_info {
            custom.insert(
                "MCP-Client-Info".to_owned(),
                encode_header_json(&serde_json::to_value(info).expect("serializable client info")),
            );
        }
        Ok(Self {
            protocol: Some(profile.revision().to_owned()),
            method: rule.requires_method_header.then(|| rule.name.to_owned()),
            name: name.map(str::to_owned),
            x_mcp_headers: custom,
        })
    }

    /// Check received headers are unique, complete, and identical to the body projection.
    pub fn check(&self, headers: &[(String, String)]) -> Result<(), HeaderError> {
        let mut seen = BTreeMap::new();
        for (name, value) in headers {
            let key = name.to_ascii_lowercase();
            if seen.insert(key.clone(), value).is_some() {
                return Err(HeaderError::Duplicate(name.clone()));
            }
            if key.starts_with("x-mcp-") || key.starts_with("mcp-") {
                validate_header_value(value)?;
            }
        }
        check_header(&seen, "mcp-protocol-version", self.protocol.as_deref())?;
        check_header(&seen, "mcp-method", self.method.as_deref())?;
        check_header(&seen, "mcp-name", self.name.as_deref())?;
        for (name, value) in &self.x_mcp_headers {
            check_header(&seen, &name.to_ascii_lowercase(), Some(value))?;
        }
        Ok(())
    }
}

/// Wire-authority validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WireError {
    /// A setting collides with an MCP-owned or unnamespaced key.
    ExtensionCollision(String),
    /// A setting exceeded bounded JSON limits.
    ExtensionLimit(String),
    /// Extension behavior was not negotiated.
    UnnegotiatedExtension(String),
    /// Invalid result discriminator.
    InvalidResultType(String),
    /// Required projected field was absent.
    MissingProjection(&'static str),
    /// Status, discriminator, payload, or cache combination was invalid.
    InvalidStatusBody,
    /// Trace metadata was malformed.
    MalformedTrace(&'static str),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Header/body projection disagreement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HeaderError {
    /// A header appeared more than once, including case variants.
    Duplicate(String),
    /// A required header was absent.
    Missing(String),
    /// Header and body interpretations conflict.
    Conflict(String),
    /// Header value violates safe-value/base64 grammar.
    UnsafeValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    // conformance: dated MCP wire profiles keep one exact authority path.
    #[test]
    fn dated_profile_revisions_remain_exact() {
        assert_eq!(ProtocolProfileId::Modern20260728.revision(), MODERN);
        assert_eq!(ProtocolProfileId::Legacy20250326.revision(), LEGACY);
    }
}
