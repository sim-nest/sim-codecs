// conformance: projector admission closes imports and resets mutable guest state.

use std::collections::BTreeSet;

use sim_kernel::Symbol;

use crate::{
    DeterministicWasmImport, Frame, ProjectionModuleError, ProjectionModulePolicy, WasmFrameLimits,
    WasmiProjectionRuntime, inspect_projection_module,
};

fn policy(imports: impl IntoIterator<Item = DeterministicWasmImport>) -> ProjectionModulePolicy {
    ProjectionModulePolicy {
        imports: imports.into_iter().collect(),
        allow_start: false,
        limits: WasmFrameLimits::default(),
    }
}

#[test]
fn exact_deterministic_import_manifest_qualifies() {
    let bytes = wat::parse_str(
        r#"(module (import "projection.input" "get" (func (param i32) (result i32))))"#,
    )
    .unwrap();
    let import = DeterministicWasmImport::new("projection.input", "get");
    let admission = inspect_projection_module(&bytes, &policy([import.clone()])).unwrap();
    assert_eq!(admission.imports, BTreeSet::from([import]));
    assert!(!admission.has_start);
}

#[test]
fn clock_random_and_wasi_imports_fail_even_when_declared() {
    for (module, name) in [
        ("wasi:clocks/wall-clock", "now"),
        ("wasi:random/random", "get-random-bytes"),
        ("wasi_snapshot_preview1", "path_open"),
    ] {
        let bytes =
            wat::parse_str(format!("(module (import \"{module}\" \"{name}\" (func)))")).unwrap();
        let import = DeterministicWasmImport::new(module, name);
        assert_eq!(
            inspect_projection_module(&bytes, &policy([import.clone()])),
            Err(ProjectionModuleError::ForbiddenImport(import))
        );
    }
}

#[test]
fn declared_but_absent_and_present_but_undeclared_imports_are_distinct() {
    let actual = DeterministicWasmImport::new("projection.input", "get");
    let declared = DeterministicWasmImport::new("projection.input", "list");
    let bytes = wat::parse_str(
        r#"(module (import "projection.input" "get" (func (param i32) (result i32))))"#,
    )
    .unwrap();
    assert_eq!(
        inspect_projection_module(&bytes, &policy([declared.clone()])),
        Err(ProjectionModuleError::ImportManifestMismatch {
            missing: BTreeSet::from([declared]),
            undeclared: BTreeSet::from([actual]),
        })
    );
}

#[test]
fn start_and_invalid_limits_fail_before_instantiation() {
    let bytes = wat::parse_str("(module (func $start) (start $start))").unwrap();
    assert_eq!(
        inspect_projection_module(&bytes, &policy([])),
        Err(ProjectionModuleError::StartNotAllowed)
    );
    let mut invalid = policy([]);
    invalid.limits.fuel_per_call = 0;
    assert_eq!(
        inspect_projection_module(&bytes, &invalid),
        Err(ProjectionModuleError::InvalidLimits)
    );
}

#[test]
fn projection_driver_resets_instance_state_for_every_call() {
    let bytes = wat::parse_str(
        r#"
        (module
          (memory (export "memory") 1)
          (global $counter (mut i32) (i32.const 0))
          (global $heap (mut i32) (i32.const 1024))
          (func (export "sim_alloc") (param $len i32) (result i32)
            (local $old i32)
            global.get $heap
            local.tee $old
            local.get $len
            i32.add
            global.set $heap
            local.get $old)
          (func (export "sim_manifest") (result i64) (i64.const 0))
          (func (export "sim_exports") (result i64) (i64.const 0))
          (func (export "sim_call") (param i32 i32 i32 i32) (result i64)
            global.get $counter
            i32.const 1
            i32.add
            global.set $counter
            i32.const 0
            global.get $counter
            i32.store8
            i64.const 4294967296))
        "#,
    )
    .unwrap();
    let runtime = WasmiProjectionRuntime::new(WasmFrameLimits::default()).unwrap();
    let function = Symbol::qualified("projection", "run");
    let first = runtime
        .call_fresh(&bytes, &function, Frame::empty())
        .unwrap();
    let second = runtime
        .call_fresh(&bytes, &function, Frame::empty())
        .unwrap();
    assert_eq!(first.bytes(), &[1]);
    assert_eq!(second.bytes(), &[1]);
}
