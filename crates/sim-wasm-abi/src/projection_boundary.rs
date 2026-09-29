//! Owner-verified compiled Wasm specimens for projection admission.

use crate::{
    DeterministicWasmImport, Frame, ProjectionModuleError, ProjectionModulePolicy, WasmFrameLimits,
    WasmiProjectionRuntime, inspect_projection_module,
};
use sim_kernel::{ContentId, Datum, Symbol};
use std::{collections::BTreeMap, error::Error, fmt};

const FACTS: [&str; 6] = [
    "boundary.wasm-import-manifest-complete",
    "boundary.wasm-imports-exact",
    "boundary.wasm-clock-random-wasi-refused",
    "boundary.wasm-runtime-semantics-qualified",
    "boundary.wasm-instance-fresh",
    "boundary.wasm-budgets-bound",
];

/// Opaque result of executing the fixed compiled Wasm boundary specimens.
///
/// Construction stays inside this crate. It records identities of actual Wasm
/// bytes and observed runtime results, never caller-authored Boolean claims.
pub struct VerifiedProjectionWasmBoundary {
    identity: ContentId,
    canonical: Datum,
    witnesses: BTreeMap<&'static str, ContentId>,
}

impl VerifiedProjectionWasmBoundary {
    /// Identity of the complete compiled and executed specimen set.
    #[must_use]
    pub const fn identity(&self) -> &ContentId {
        &self.identity
    }

    /// Canonical evidence projection; it grants no runtime authority.
    #[must_use]
    pub const fn canonical_datum(&self) -> &Datum {
        &self.canonical
    }

    /// Exact compiled-run support for one projection checker fact.
    #[must_use]
    pub fn witness(&self, fact: &str) -> Option<&ContentId> {
        self.witnesses.get(fact)
    }
}

impl fmt::Debug for VerifiedProjectionWasmBoundary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedProjectionWasmBoundary")
            .field("identity", &self.identity)
            .field("witnesses", &self.witnesses.len())
            .finish_non_exhaustive()
    }
}

/// Refusal from compiling, inspecting, or executing a fixed Wasm specimen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionWasmBoundaryError(String);

impl fmt::Display for ProjectionWasmBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ProjectionWasmBoundaryError {}

/// Compiles and executes the fixed deterministic and hostile Wasm corpus.
///
/// The successful module is instantiated twice. Separate compiled modules test
/// exact imports, clock/random/WASI refusal, forbidden start behavior, finite
/// fuel, and finite memory growth. Any unexpected success or unrelated failure
/// refuses the whole corpus.
///
/// # Errors
///
/// Returns the exact failed specimen stage. Partial results are never exposed.
pub fn verify_projection_wasm_boundary()
-> Result<VerifiedProjectionWasmBoundary, ProjectionWasmBoundaryError> {
    let exact_import = compile(
        "exact-import",
        r#"(module (import "projection.input" "get" (func (param i32) (result i32))))"#,
    )?;
    let exact = DeterministicWasmImport::new("projection.input", "get");
    let import_admission = inspect_projection_module(
        &exact_import,
        &policy([exact.clone()], WasmFrameLimits::default()),
    )
    .map_err(|error| refused("exact-import", error))?;
    if import_admission.imports != [exact].into_iter().collect() || import_admission.has_start {
        return Err(invalid("exact import observation differs"));
    }

    let mut forbidden_ids = Vec::new();
    for (label, module, name) in [
        ("clock", "wasi:clocks/wall-clock", "now"),
        ("random", "wasi:random/random", "get-random-bytes"),
        ("wasi", "wasi_snapshot_preview1", "path_open"),
    ] {
        let bytes = compile(
            label,
            &format!("(module (import \"{module}\" \"{name}\" (func)))"),
        )?;
        let selected = DeterministicWasmImport::new(module, name);
        if inspect_projection_module(
            &bytes,
            &policy([selected.clone()], WasmFrameLimits::default()),
        ) != Err(ProjectionModuleError::ForbiddenImport(selected))
        {
            return Err(invalid("forbidden Wasm import was not identified exactly"));
        }
        forbidden_ids.push(module_id(&bytes)?);
    }

    let start = compile("start", "(module (func $start) (start $start))")?;
    if inspect_projection_module(&start, &policy([], WasmFrameLimits::default()))
        != Err(ProjectionModuleError::StartNotAllowed)
    {
        return Err(invalid("Wasm start behavior was not refused"));
    }

    let success = compile("fresh-instance", SUCCESS_MODULE)?;
    inspect_projection_module(&success, &policy([], WasmFrameLimits::default()))
        .map_err(|error| refused("fresh-instance-inspection", error))?;
    let runtime = WasmiProjectionRuntime::new(WasmFrameLimits::default())
        .map_err(|error| refused("fresh-instance-runtime", error))?;
    let function = Symbol::qualified("projection", "run");
    let first = runtime
        .call_fresh(&success, &function, Frame::empty())
        .map_err(|error| refused("fresh-instance-first", error))?;
    let second = runtime
        .call_fresh(&success, &function, Frame::empty())
        .map_err(|error| refused("fresh-instance-second", error))?;
    if first.bytes() != [1] || second.bytes() != [1] {
        return Err(invalid("fresh Wasm instances did not reset state"));
    }

    let fuel = compile("fuel", FUEL_MODULE)?;
    let fuel_limits = WasmFrameLimits {
        fuel_per_call: 100_000,
        ..WasmFrameLimits::default()
    };
    inspect_projection_module(&fuel, &policy([], fuel_limits))
        .map_err(|error| refused("fuel-inspection", error))?;
    let fuel_result = WasmiProjectionRuntime::new(fuel_limits)
        .map_err(|error| refused("fuel-runtime", error))?
        .call_fresh(&fuel, &function, Frame::empty());
    let Err(fuel_error) = fuel_result else {
        return Err(invalid("infinite Wasm specimen exceeded its fuel budget"));
    };
    if !fuel_error.to_string().to_ascii_lowercase().contains("fuel") {
        return Err(invalid(
            "infinite Wasm specimen failed outside fuel enforcement",
        ));
    }

    let memory = compile("memory", MEMORY_MODULE)?;
    let memory_limits = WasmFrameLimits {
        max_frame_bytes: 64 * 1024,
        max_memory_bytes: 2 * 1024 * 1024,
        ..WasmFrameLimits::default()
    };
    inspect_projection_module(&memory, &policy([], memory_limits))
        .map_err(|error| refused("memory-inspection", error))?;
    let memory_result = WasmiProjectionRuntime::new(memory_limits)
        .map_err(|error| refused("memory-runtime", error))?
        .call_fresh(&memory, &function, Frame::empty());
    if memory_result.is_ok() {
        return Err(invalid("Wasm memory growth exceeded its configured bound"));
    }

    let import_id = module_id(&exact_import)?;
    let start_id = module_id(&start)?;
    let success_id = module_id(&success)?;
    let fuel_id = module_id(&fuel)?;
    let memory_id = module_id(&memory)?;
    let forbidden = aggregate("forbidden-imports", &forbidden_ids)?;
    let budgets = aggregate("runtime-budgets", &[fuel_id, memory_id])?;
    let fresh = aggregate("fresh-instance", std::slice::from_ref(&success_id))?;
    let mut witnesses = BTreeMap::new();
    witnesses.insert(FACTS[0], import_id.clone());
    witnesses.insert(FACTS[1], import_id);
    witnesses.insert(FACTS[2], forbidden);
    witnesses.insert(FACTS[3], success_id);
    witnesses.insert(FACTS[4], fresh);
    witnesses.insert(FACTS[5], budgets);
    let canonical = Datum::Node {
        tag: Symbol::qualified("wasm", "verified-projection-boundary-v1"),
        fields: vec![
            (
                Symbol::new("witnesses"),
                Datum::Vector(
                    witnesses
                        .iter()
                        .map(|(fact, support)| {
                            Datum::Vector(vec![
                                Datum::String((*fact).to_owned()),
                                id_datum(support),
                            ])
                        })
                        .collect(),
                ),
            ),
            (Symbol::new("start-refusal"), id_datum(&start_id)),
        ],
    };
    let identity = canonical
        .content_id()
        .map_err(|error| invalid(error.to_string()))?;
    Ok(VerifiedProjectionWasmBoundary {
        identity,
        canonical,
        witnesses,
    })
}

fn policy(
    imports: impl IntoIterator<Item = DeterministicWasmImport>,
    limits: WasmFrameLimits,
) -> ProjectionModulePolicy {
    ProjectionModulePolicy {
        imports: imports.into_iter().collect(),
        allow_start: false,
        limits,
    }
}

fn compile(label: &str, source: &str) -> Result<Vec<u8>, ProjectionWasmBoundaryError> {
    wat::parse_str(source).map_err(|error| invalid(format!("{label} compile: {error}")))
}

fn module_id(bytes: &[u8]) -> Result<ContentId, ProjectionWasmBoundaryError> {
    Datum::Node {
        tag: Symbol::qualified("wasm", "compiled-projection-specimen-v1"),
        fields: vec![(Symbol::new("bytes"), Datum::Bytes(bytes.to_vec()))],
    }
    .content_id()
    .map_err(|error| invalid(error.to_string()))
}

fn aggregate(role: &str, members: &[ContentId]) -> Result<ContentId, ProjectionWasmBoundaryError> {
    Datum::Node {
        tag: Symbol::qualified("wasm", "projection-specimen-group-v1"),
        fields: vec![
            (Symbol::new("role"), Datum::String(role.to_owned())),
            (
                Symbol::new("members"),
                Datum::Vector(members.iter().map(id_datum).collect()),
            ),
        ],
    }
    .content_id()
    .map_err(|error| invalid(error.to_string()))
}

fn id_datum(identity: &ContentId) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("wasm", "content-id-v1"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::Symbol(identity.algorithm.clone()),
            ),
            (Symbol::new("digest"), Datum::Bytes(identity.bytes.to_vec())),
        ],
    }
}

fn refused(label: &str, error: ProjectionModuleError) -> ProjectionWasmBoundaryError {
    invalid(format!("{label}: {error}"))
}

fn invalid(message: impl Into<String>) -> ProjectionWasmBoundaryError {
    ProjectionWasmBoundaryError(message.into())
}

const SUCCESS_MODULE: &str = r#"
(module
  (memory (export "memory") 1)
  (global $counter (mut i32) (i32.const 0))
  (global $heap (mut i32) (i32.const 1024))
  (func (export "sim_alloc") (param $len i32) (result i32)
    (local $old i32)
    global.get $heap local.tee $old local.get $len i32.add global.set $heap local.get $old)
  (func (export "sim_manifest") (result i64) (i64.const 0))
  (func (export "sim_exports") (result i64) (i64.const 0))
  (func (export "sim_call") (param i32 i32 i32 i32) (result i64)
    global.get $counter i32.const 1 i32.add global.set $counter
    i32.const 0 global.get $counter i32.store8 i64.const 4294967296))
"#;

const FUEL_MODULE: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "sim_alloc") (param i32) (result i32) i32.const 0)
  (func (export "sim_manifest") (result i64) i64.const 0)
  (func (export "sim_exports") (result i64) i64.const 0)
  (func (export "sim_call") (param i32 i32 i32 i32) (result i64)
    (loop $l (br $l)) unreachable))
"#;

const MEMORY_MODULE: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "sim_alloc") (param i32) (result i32) i32.const 0)
  (func (export "sim_manifest") (result i64) i64.const 0)
  (func (export "sim_exports") (result i64) i64.const 0)
  (func (export "sim_call") (param i32 i32 i32 i32) (result i64)
    i32.const 1000 memory.grow drop i64.const 0))
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_compiled_projection_boundary_is_complete_and_repeatable() {
        let first = verify_projection_wasm_boundary().unwrap();
        let second = verify_projection_wasm_boundary().unwrap();
        assert_eq!(first.identity(), second.identity());
        assert_eq!(first.canonical_datum(), second.canonical_datum());
        for fact in FACTS {
            assert!(first.witness(fact).is_some(), "missing {fact}");
        }
        assert!(first.witness("boundary.wasm-self-declared").is_none());
    }
}
