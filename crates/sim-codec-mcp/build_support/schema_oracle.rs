use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    revision: String,
    definitions: Vec<Definition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Definition {
    source_path: String,
    kind: String,
    wire_name: String,
    #[serde(default)]
    open_extension_reason: Option<String>,
}

#[derive(Deserialize)]
struct Ledger {
    schema: String,
    revision: String,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    source_path: String,
    rust_owner: String,
    positive_vector: String,
    negative_vector: String,
    owning_phase: String,
    #[serde(default)]
    open_extension_reason: Option<String>,
}

pub fn validate_and_generate(manifest: &Path, output: &Path) -> Result<(), String> {
    let root = manifest.join("fixtures/mcp/2026-07-28");
    let schema: Inventory = read_json(&root.join("schema.json"))?;
    let ledger: Ledger = read_json(&root.join("coverage.json"))?;
    let provenance: serde_json::Value = read_json(&root.join("provenance.json"))?;
    for field in ["sourceUrl", "revisionLabel", "acquiredOn", "license"] {
        if provenance[field].as_str().is_none_or(str::is_empty) {
            return Err(format!("missing schema provenance field: {field}"));
        }
    }
    let vectors: serde_json::Value = read_json(&root.join("vectors.json"))?;
    let vector_ids: BTreeSet<_> = vectors["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|case| case["id"].as_str())
        .collect();
    for required in [
        "required-metadata",
        "legacy-absent-result-type",
        "extension-result-type",
        "server-info",
        "header-safe-base64",
        "header-mismatch",
        "unsupported-protocol-version",
        "missing-capability",
        "tracing",
        "mrtr",
        "subscriptions",
        "caching",
        "discovery",
    ] {
        if !vector_ids.contains(required) {
            return Err(format!("missing required pinned vector: {required}"));
        }
    }
    let fixture_bytes = directory_bytes(&manifest.join("fixtures/mcp"))?;
    if fixture_bytes > 64 * 1024 {
        return Err(format!(
            "MCP oracle fixture bound exceeded: {fixture_bytes} > 65536 bytes"
        ));
    }
    if schema.revision != "2026-07-28" || ledger.revision != schema.revision {
        return Err("coverage profile revision mismatch: expected 2026-07-28".into());
    }
    if ledger.schema != "sim.mcp-schema-coverage/v1" {
        return Err(format!(
            "unsupported coverage ledger schema: {}",
            ledger.schema
        ));
    }

    let allowed = BTreeSet::from([
        "method",
        "notification",
        "request",
        "result",
        "error",
        "meta",
        "header",
        "result-type",
        "extension",
        "cache-hint",
        "trace-field",
        "shape",
    ]);
    let mut definitions = BTreeMap::new();
    for definition in &schema.definitions {
        if !allowed.contains(definition.kind.as_str()) {
            return Err(format!(
                "unclassified schema definition: {}",
                definition.source_path
            ));
        }
        if definition.kind == "extension"
            && definition
                .open_extension_reason
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Err(format!("unclassified open map: {}", definition.source_path));
        }
        if definitions
            .insert(&definition.source_path, definition)
            .is_some()
        {
            return Err(format!(
                "duplicated schema definition: {}",
                definition.source_path
            ));
        }
    }
    let mut entries = BTreeMap::new();
    for entry in &ledger.entries {
        if entries.insert(&entry.source_path, entry).is_some() {
            return Err(format!(
                "duplicated coverage ledger row: {}",
                entry.source_path
            ));
        }
    }
    for (path, definition) in &definitions {
        let Some(entry) = entries.get(path) else {
            return Err(format!("uncovered source path: {path}"));
        };
        if entry.rust_owner.is_empty() || entry.owning_phase.is_empty() {
            return Err(format!("incomplete coverage ledger row: {path}"));
        }
        if definition.kind == "extension"
            && entry.open_extension_reason != definition.open_extension_reason
        {
            return Err(format!("open-extension classification mismatch: {path}"));
        }
        for vector in [&entry.positive_vector, &entry.negative_vector] {
            let vector_path = manifest.join(vector);
            if !vector_path.is_file() {
                return Err(format!("missing coverage vector for {path}: {vector}"));
            }
            let _: serde_json::Value = read_json(&vector_path)?;
        }
    }
    for path in entries.keys() {
        if !definitions.contains_key(path) {
            return Err(format!("ledger row has no schema definition: {path}"));
        }
    }
    let schema_bytes = fs::read(root.join("schema.json")).map_err(|error| error.to_string())?;
    let actual_digest = format!("{:x}", Sha256::digest(&schema_bytes));
    if provenance["normalizedSchemaSha256"].as_str() != Some(&actual_digest) {
        return Err(format!(
            "pinned schema digest mismatch: expected {actual_digest}"
        ));
    }

    let mut generated =
        String::from("/// Wire vocabulary generated from the pinned 2026-07-28 schema.\n");
    generated.push_str("pub const MCP_2026_07_28_VOCABULARY: &[(&str, &str)] = &[\n");
    for definition in &schema.definitions {
        generated.push_str(&format!(
            "    ({:?}, {:?}),\n",
            definition.source_path, definition.wire_name
        ));
    }
    generated.push_str("];\n");
    fs::write(output.join("mcp_vocabulary.rs"), generated)
        .map_err(|error| format!("write generated MCP vocabulary: {error}"))?;
    Ok(())
}

fn directory_bytes(path: &Path) -> Result<u64, String> {
    let mut total = 0;
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        total += if metadata.is_dir() {
            directory_bytes(&entry.path())?
        } else {
            metadata.len()
        };
    }
    Ok(total)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse {}: {error}", path.display()))
}
