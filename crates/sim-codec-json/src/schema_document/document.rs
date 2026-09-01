use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ShapeSchema;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The supported JSON Schema dialect.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Compile and validation resource limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaLimits {
    /// Maximum source document bytes.
    pub document_bytes: usize,
    /// Maximum nesting in a schema.
    pub schema_depth: usize,
    /// Maximum nesting in an instance.
    pub instance_depth: usize,
    /// Maximum reference resolutions.
    pub refs: usize,
    /// Maximum evaluated composition branches.
    pub evaluated_branches: usize,
    /// Maximum approximate regular-expression work.
    pub regex_work: usize,
    /// Maximum returned diagnostics.
    pub errors: usize,
    /// Maximum bytes across diagnostic messages.
    pub diagnostic_bytes: usize,
}

impl Default for SchemaLimits {
    fn default() -> Self {
        Self {
            document_bytes: 1 << 20,
            schema_depth: 128,
            instance_depth: 128,
            refs: 256,
            evaluated_branches: 4096,
            regex_work: 1 << 20,
            errors: 32,
            diagnostic_bytes: 16 << 10,
        }
    }
}

/// Explicit identity of a schema resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceIdentity {
    /// Absolute or caller-defined base URI.
    pub base_uri: String,
    /// Stable source label suitable for diagnostics.
    pub source: String,
}

/// Bytes returned by an explicitly injected retriever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedResource {
    /// Resource bytes.
    pub bytes: Vec<u8>,
    /// Declared media type.
    pub media_type: String,
    /// Final base URI.
    pub base_uri: String,
    /// Expected semantic digest.
    pub digest: String,
    /// Explicit redirect chain; empty means no redirects.
    pub redirects: Vec<String>,
}

/// An application-owned, bounded resource resolver. No ambient network or
/// filesystem resolver exists in this crate.
pub trait SchemaRetriever {
    /// Retrieve one exact URI under application policy.
    fn retrieve(&self, uri: &str) -> Result<RetrievedResource, SchemaError>;
}

/// A parsed schema preserving the complete semantic JSON document.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaDocument {
    value: Value,
    dialect: String,
    digest: String,
    identity: ResourceIdentity,
    limits: SchemaLimits,
}

impl SchemaDocument {
    /// Parse and compile a Draft 2020-12 schema. Boolean schemas are supported.
    pub fn parse(
        bytes: &[u8],
        identity: ResourceIdentity,
        limits: SchemaLimits,
    ) -> Result<Self, SchemaError> {
        if bytes.len() > limits.document_bytes {
            return Err(SchemaError::compile(
                &identity.source,
                "",
                "budget",
                "schema document byte budget exceeded",
            ));
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|e| SchemaError::compile(&identity.source, "", "parse", &e.to_string()))?;
        Self::from_value(value, identity, limits)
    }

    /// Compile an already parsed value.
    pub fn from_value(
        value: Value,
        identity: ResourceIdentity,
        limits: SchemaLimits,
    ) -> Result<Self, SchemaError> {
        if !value.is_object() && !value.is_boolean() {
            return Err(SchemaError::compile(
                &identity.source,
                "",
                "schema",
                "schema must be an object or boolean",
            ));
        }
        let dialect = value
            .get("$schema")
            .and_then(Value::as_str)
            .unwrap_or(DRAFT_2020_12)
            .to_owned();
        if dialect != DRAFT_2020_12 && dialect != format!("{DRAFT_2020_12}#") {
            return Err(SchemaError::compile(
                &identity.source,
                "/$schema",
                "$schema",
                "unsupported schema dialect",
            ));
        }
        check_schema(&value, 0, "", &identity.source, limits)?;
        let digest = semantic_digest(&value);
        Ok(Self {
            value,
            dialect,
            digest,
            identity,
            limits,
        })
    }

    /// Complete parsed JSON, including all annotations and extensions.
    pub fn value(&self) -> &Value {
        &self.value
    }
    /// Selected or default dialect.
    pub fn dialect(&self) -> &str {
        &self.dialect
    }
    /// Stable digest of canonical semantic JSON (object key order is ignored).
    pub fn semantic_digest(&self) -> &str {
        &self.digest
    }
    /// Resource identity.
    pub fn identity(&self) -> &ResourceIdentity {
        &self.identity
    }
    /// Compile/validation limits.
    pub fn limits(&self) -> SchemaLimits {
        self.limits
    }

    /// Adapt the schema to the closed local Shape projection when every
    /// authoritative constraint is representable. `None` means callers must
    /// retain and use this document; it never means an unconstrained Shape.
    pub fn shape_projection(&self) -> Option<ShapeSchema> {
        project_shape(&self.value)
    }

    /// Validate without external resources. Non-local references fail closed.
    pub fn validate(&self, instance: &Value) -> Result<(), Vec<SchemaError>> {
        self.validate_with(instance, None)
    }

    /// Validate using only the supplied retriever for non-local resources.
    pub fn validate_with(
        &self,
        instance: &Value,
        retriever: Option<&dyn SchemaRetriever>,
    ) -> Result<(), Vec<SchemaError>> {
        let mut state = State::new(self, retriever);
        state.eval(&self.value, instance, "", "", 0);
        if state.errors.is_empty() {
            Ok(())
        } else {
            Err(state.errors)
        }
    }
}

/// Stable, bounded validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    /// RFC 6901 instance pointer.
    pub instance_pointer: String,
    /// RFC 6901 schema pointer.
    pub schema_pointer: String,
    /// Failing keyword.
    pub keyword: String,
    /// Bounded non-secret message.
    pub message: String,
    /// Stable schema source identity.
    pub source: String,
}

impl SchemaError {
    fn compile(source: &str, schema: &str, keyword: &str, message: &str) -> Self {
        Self {
            instance_pointer: String::new(),
            schema_pointer: schema.into(),
            keyword: keyword.into(),
            message: message.chars().take(512).collect(),
            source: source.into(),
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at instance {} (schema {}, source {})",
            self.message, self.instance_pointer, self.schema_pointer, self.source
        )
    }
}
impl std::error::Error for SchemaError {}

fn check_schema(
    v: &Value,
    depth: usize,
    ptr: &str,
    source: &str,
    limits: SchemaLimits,
) -> Result<(), SchemaError> {
    if depth > limits.schema_depth {
        return Err(SchemaError::compile(
            source,
            ptr,
            "budget",
            "schema depth budget exceeded",
        ));
    }
    let Some(o) = v.as_object() else {
        return if v.is_boolean() {
            Ok(())
        } else {
            Err(SchemaError::compile(
                source,
                ptr,
                "schema",
                "subschema must be an object or boolean",
            ))
        };
    };
    if let Some(r) = o.get("$ref").and_then(Value::as_str)
        && r.is_empty()
    {
        return Err(SchemaError::compile(
            source,
            &format!("{ptr}/$ref"),
            "$ref",
            "empty reference is unsupported",
        ));
    }
    if let Some(pattern) = o.get("pattern") {
        let pattern = pattern.as_str().ok_or_else(|| {
            SchemaError::compile(
                source,
                &format!("{ptr}/pattern"),
                "pattern",
                "keyword must be a string",
            )
        })?;
        EcmaPattern::compile(pattern).map_err(|error| {
            SchemaError::compile(
                source,
                &format!("{ptr}/pattern"),
                "pattern",
                error.compile_message(),
            )
        })?;
    }
    for key in [
        "properties",
        "$defs",
        "patternProperties",
        "dependentSchemas",
    ] {
        if let Some(map) = o.get(key) {
            let map = map.as_object().ok_or_else(|| {
                SchemaError::compile(
                    source,
                    &format!("{ptr}/{key}"),
                    key,
                    "keyword must be an object",
                )
            })?;
            for (name, sub) in map {
                if key == "patternProperties" {
                    EcmaPattern::compile(name).map_err(|error| {
                        SchemaError::compile(
                            source,
                            &format!("{ptr}/{key}/{}", escape(name)),
                            key,
                            error.compile_message(),
                        )
                    })?;
                }
                check_schema(
                    sub,
                    depth + 1,
                    &format!("{ptr}/{key}/{}", escape(name)),
                    source,
                    limits,
                )?;
            }
        }
    }
    for key in [
        "items",
        "contains",
        "not",
        "if",
        "then",
        "else",
        "additionalProperties",
        "unevaluatedProperties",
        "propertyNames",
    ] {
        if let Some(sub) = o.get(key) {
            check_schema(sub, depth + 1, &format!("{ptr}/{key}"), source, limits)?;
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(items) = o.get(key) {
            let items = items.as_array().ok_or_else(|| {
                SchemaError::compile(
                    source,
                    &format!("{ptr}/{key}"),
                    key,
                    "keyword must be an array",
                )
            })?;
            for (i, sub) in items.iter().enumerate() {
                check_schema(sub, depth + 1, &format!("{ptr}/{key}/{i}"), source, limits)?;
            }
        }
    }
    Ok(())
}
