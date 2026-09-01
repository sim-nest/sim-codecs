//! Pinned MCP protocol-schema coverage oracle.

use serde::Deserialize;

/// One classified source definition from the normalized dated schema.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchemaDefinition {
    /// Stable JSON Pointer into the official dated source vocabulary.
    pub source_path: String,
    /// Protocol category used to reject unclassified definitions.
    pub kind: String,
    /// Wire spelling generated for consumers.
    pub wire_name: String,
    /// Why an intentionally open map is safe; absent for closed definitions.
    #[serde(default)]
    pub open_extension_reason: Option<String>,
}

/// Minimal normalized inventory derived from one immutable official revision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInventory {
    /// Exact protocol revision.
    pub revision: String,
    /// Classified definitions in source order.
    pub definitions: Vec<SchemaDefinition>,
}

/// Exhaustive mapping and vector ownership for one schema definition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoverageEntry {
    /// Source JSON Pointer.
    pub source_path: String,
    /// Existing or planned canonical Rust owner, never a parallel wire model.
    pub rust_owner: String,
    /// Positive vector path.
    pub positive_vector: String,
    /// Negative vector path.
    pub negative_vector: String,
    /// Roadmap phase owning delivery of the mapped behavior.
    pub owning_phase: String,
    /// Explicit classification of any open map.
    #[serde(default)]
    pub open_extension_reason: Option<String>,
}

/// Machine-readable schema-to-code coverage ledger.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CoverageLedger {
    /// Ledger format identifier.
    pub schema: String,
    /// Exact source revision covered.
    pub revision: String,
    /// One and only one row per schema definition.
    pub entries: Vec<CoverageEntry>,
}

/// A protocol profile SIM may advertise or decode at a version boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolProfile {
    /// Exact dated revision, never an inferred alias.
    pub revision: &'static str,
    /// Whether the profile is the stateless final protocol.
    pub modern: bool,
}

/// The checked modern schema inventory.
pub fn modern_schema() -> SchemaInventory {
    serde_json::from_str(include_str!("../fixtures/mcp/2026-07-28/schema.json"))
        .expect("build-time checked MCP schema")
}

/// The checked exhaustive coverage ledger.
pub fn coverage_ledger() -> CoverageLedger {
    serde_json::from_str(include_str!("../fixtures/mcp/2026-07-28/coverage.json"))
        .expect("build-time checked MCP coverage ledger")
}

/// Exactly the delivered legacy profile and the pinned final profile.
pub const fn protocol_profiles() -> [ProtocolProfile; 2] {
    [
        ProtocolProfile {
            revision: "2025-03-26",
            modern: false,
        },
        ProtocolProfile {
            revision: "2026-07-28",
            modern: true,
        },
    ]
}

include!(concat!(env!("OUT_DIR"), "/mcp_vocabulary.rs"));
