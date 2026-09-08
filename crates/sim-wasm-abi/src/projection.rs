//! Closed deterministic wasm admission and fresh-instance projection driving.

use std::{collections::BTreeSet, error::Error, fmt};

use sim_kernel::{ContentId, Datum, Symbol};
use wasmi::{Config, Engine, Module};
use wasmparser::{Parser, Payload};

use crate::{Frame, WasmFrameLimits, WasmHost, WasmRuntime, WasmiRuntime};

/// One exact deterministic import admitted for a projection module.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeterministicWasmImport {
    /// Wasm import module name.
    pub module: String,
    /// Wasm import field name.
    pub name: String,
}

impl DeterministicWasmImport {
    /// Constructs an exact import pair.
    #[must_use]
    pub fn new(module: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
        }
    }

    /// Returns the stable `module/name` spelling.
    #[must_use]
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.module, self.name)
    }
}

/// Complete policy for admitting one deterministic projection module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionModulePolicy {
    /// Exact complete import set.
    pub imports: BTreeSet<DeterministicWasmImport>,
    /// Whether a module-level start function is allowed.
    pub allow_start: bool,
    /// Fuel and memory/table limits used by fresh invocations.
    pub limits: WasmFrameLimits,
}

/// Checked, content-addressed module admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionModuleAdmission {
    /// Semantic identity of the exact wasm bytes.
    pub module: ContentId,
    /// Complete discovered import set.
    pub imports: BTreeSet<DeterministicWasmImport>,
    /// Whether the module declares a start section.
    pub has_start: bool,
    /// Runtime behavior fixed by this admission route.
    pub semantics: Symbol,
    /// Fuel and memory/table limits bound to execution.
    pub limits: WasmFrameLimits,
}

/// Typed closed-wasm admission refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectionModuleError {
    /// Module bytes were malformed or unsupported.
    InvalidModule(String),
    /// The complete actual import set differs from policy.
    ImportManifestMismatch {
        /// Imports required by policy but absent from the module.
        missing: BTreeSet<DeterministicWasmImport>,
        /// Imports present in the module but absent from policy.
        undeclared: BTreeSet<DeterministicWasmImport>,
    },
    /// An ambient or nondeterministic import is forbidden even if declared.
    ForbiddenImport(DeterministicWasmImport),
    /// A start function was present without explicit admission.
    StartNotAllowed,
    /// A configured execution bound is zero or inconsistent.
    InvalidLimits,
    /// Guest execution failed after admission.
    Execution(String),
}

impl fmt::Display for ProjectionModuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ProjectionModuleError {}

/// Inspects exact module bytes without instantiating or running guest code.
pub fn inspect_projection_module(
    bytes: &[u8],
    policy: &ProjectionModulePolicy,
) -> Result<ProjectionModuleAdmission, ProjectionModuleError> {
    validate_limits(policy.limits)?;
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, bytes)
        .map_err(|error| ProjectionModuleError::InvalidModule(error.to_string()))?;
    let imports = module
        .imports()
        .map(|import| DeterministicWasmImport::new(import.module(), import.name()))
        .collect::<BTreeSet<_>>();
    let has_start = Parser::new(0)
        .parse_all(bytes)
        .try_fold(false, |found, payload| {
            let payload =
                payload.map_err(|error| ProjectionModuleError::InvalidModule(error.to_string()))?;
            Ok::<_, ProjectionModuleError>(found || matches!(payload, Payload::StartSection { .. }))
        })?;
    for import in &imports {
        if forbidden(import) {
            return Err(ProjectionModuleError::ForbiddenImport(import.clone()));
        }
    }
    if imports != policy.imports {
        return Err(ProjectionModuleError::ImportManifestMismatch {
            missing: policy.imports.difference(&imports).cloned().collect(),
            undeclared: imports.difference(&policy.imports).cloned().collect(),
        });
    }
    if has_start && !policy.allow_start {
        return Err(ProjectionModuleError::StartNotAllowed);
    }
    let module = Datum::Node {
        tag: Symbol::qualified("wasm", "semantic-module-v1"),
        fields: vec![(Symbol::new("bytes"), Datum::Bytes(bytes.to_vec()))],
    }
    .content_id()
    .map_err(|error| ProjectionModuleError::InvalidModule(error.to_string()))?;
    Ok(ProjectionModuleAdmission {
        module,
        imports,
        has_start,
        semantics: Symbol::qualified("wasm", "projection-runtime-v1"),
        limits: policy.limits,
    })
}

/// Wasmi projection driver that creates and drops a fresh runtime per call.
#[derive(Clone, Copy, Debug)]
pub struct WasmiProjectionRuntime {
    limits: WasmFrameLimits,
}

impl WasmiProjectionRuntime {
    /// Creates a projection runtime with admitted execution limits.
    pub fn new(limits: WasmFrameLimits) -> Result<Self, ProjectionModuleError> {
        validate_limits(limits)?;
        Ok(Self { limits })
    }

    /// Instantiates exact bytes, calls one function once, and drops all state.
    pub fn call_fresh(
        &self,
        bytes: &[u8],
        function: &Symbol,
        args: Frame,
    ) -> Result<Frame, ProjectionModuleError> {
        let runtime = WasmiRuntime::with_limits(self.limits);
        let handle = runtime
            .instantiate_bytes(bytes)
            .map_err(|error| ProjectionModuleError::Execution(error.to_string()))?;
        runtime
            .call(handle, function, args)
            .map_err(|error| ProjectionModuleError::Execution(error.to_string()))
    }
}

fn validate_limits(limits: WasmFrameLimits) -> Result<(), ProjectionModuleError> {
    if limits.max_frame_bytes == 0
        || limits.fuel_per_call == 0
        || limits.max_memory_bytes < limits.max_frame_bytes
        || limits.max_table_elements == 0
    {
        return Err(ProjectionModuleError::InvalidLimits);
    }
    Ok(())
}

fn forbidden(import: &DeterministicWasmImport) -> bool {
    const AMBIENT: &[&str] = &[
        "wasi",
        "clock",
        "time",
        "random",
        "filesystem",
        "path_",
        "proc",
        "environ",
        "network",
        "socket",
        "thread",
        "shared-memory",
    ];
    let qualified = import.qualified().to_ascii_lowercase();
    AMBIENT.iter().any(|needle| qualified.contains(needle))
}
