use super::support::{FakeGuestModule, manifest, wasm_test_cx};
use crate::runtime::pack_frame_ref;
use crate::{
    AbiValue, FrameRef, InMemoryWasmRuntime, WasmFrameLimits, WasmHost, WasmRuntime, WasmiRuntime,
    encode_exports_frame, encode_manifest_frame, encode_value_frame, load_wasm_lib_from_bytes,
    load_wasm_lib_from_file,
};
use sim_kernel::{Expr, Symbol};
use std::sync::Arc;
use wat::parse_str as wat_parse_str;

#[test]
fn in_memory_runtime_instantiates_registered_module_and_calls_through_loader() {
    let runtime = Arc::new(InMemoryWasmRuntime::new());
    runtime
        .register_module(
            b"(registered module)".to_vec(),
            Arc::new(FakeGuestModule {
                manifest: manifest(),
            }),
        )
        .unwrap();

    let lib = load_wasm_lib_from_bytes(runtime.clone(), b"(registered module)").unwrap();
    let mut cx = wasm_test_cx();
    cx.load_lib(&lib).unwrap();
    let result = cx
        .call_function(
            &Symbol::new("distance"),
            sim_kernel::Args::new(vec![
                cx.factory()
                    .number_literal(Symbol::qualified("numbers", "f64"), "1".to_owned())
                    .unwrap(),
                cx.factory()
                    .number_literal(Symbol::qualified("numbers", "f64"), "2".to_owned())
                    .unwrap(),
            ]),
        )
        .unwrap();
    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "2".to_owned(),
        })
    );
}

#[test]
fn wasmi_runtime_loads_real_wasm_module_and_calls_guest() {
    let manifest = manifest();
    let manifest_frame = encode_manifest_frame(&manifest).unwrap();
    let exports_frame = encode_exports_frame(&manifest.exports).unwrap();
    let distance_frame =
        encode_value_frame(&AbiValue::Expr(Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "2".to_owned(),
        })))
        .unwrap();
    let point_frame = encode_value_frame(&AbiValue::Expr(Expr::Map(vec![
        (
            Expr::Symbol(Symbol::new("x")),
            Expr::Number(sim_kernel::NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1".to_owned(),
            }),
        ),
        (
            Expr::Symbol(Symbol::new("y")),
            Expr::Number(sim_kernel::NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "2".to_owned(),
            }),
        ),
    ])))
    .unwrap();

    let manifest_ref = FrameRef {
        ptr: 0,
        len: manifest_frame.bytes().len() as u32,
    };
    let exports_ref = FrameRef {
        ptr: 1024,
        len: exports_frame.bytes().len() as u32,
    };
    let distance_ref = FrameRef {
        ptr: 2048,
        len: distance_frame.bytes().len() as u32,
    };
    let point_ref = FrameRef {
        ptr: 3072,
        len: point_frame.bytes().len() as u32,
    };

    let wasm_bytes = wat_parse_str(format!(
        r#"(module
            (memory (export "memory") 1)
            (global $heap (mut i32) (i32.const 4096))
            (data (i32.const 0) "{}")
            (data (i32.const 1024) "{}")
            (data (i32.const 2048) "{}")
            (data (i32.const 3072) "{}")
            (func (export "sim_alloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.tee $ptr
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)
            (func (export "sim_manifest") (result i64)
                i64.const {})
            (func (export "sim_exports") (result i64)
                i64.const {})
            (func (export "sim_call") (param $name_ptr i32) (param $name_len i32) (param $args_ptr i32) (param $args_len i32) (result i64)
                local.get $name_ptr
                i32.load8_u
                i32.const 100
                i32.eq
                if (result i64)
                    i64.const {}
                else
                    i64.const {}
                end)
        )"#,
        wat_bytes(manifest_frame.bytes()),
        wat_bytes(exports_frame.bytes()),
        wat_bytes(distance_frame.bytes()),
        wat_bytes(point_frame.bytes()),
        pack_frame_ref(manifest_ref),
        pack_frame_ref(exports_ref),
        pack_frame_ref(distance_ref),
        pack_frame_ref(point_ref),
    ))
    .unwrap();

    let runtime = Arc::new(WasmiRuntime::new());
    let lib = load_wasm_lib_from_bytes(runtime, &wasm_bytes).unwrap();
    let mut cx = wasm_test_cx();
    cx.load_lib(&lib).unwrap();

    let result = cx
        .call_function(
            &Symbol::new("distance"),
            sim_kernel::Args::new(vec![
                cx.factory()
                    .number_literal(Symbol::qualified("numbers", "f64"), "1".to_owned())
                    .unwrap(),
                cx.factory()
                    .number_literal(Symbol::qualified("numbers", "f64"), "2".to_owned())
                    .unwrap(),
            ]),
        )
        .unwrap();
    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "2".to_owned(),
        })
    );
}

#[test]
fn wasmi_runtime_exposes_minimal_host_imports() {
    let manifest = manifest();
    let manifest_frame = encode_manifest_frame(&manifest).unwrap();
    let exports_frame = encode_exports_frame(&manifest.exports).unwrap();
    let manifest_ref = FrameRef {
        ptr: 0,
        len: manifest_frame.bytes().len() as u32,
    };
    let exports_ref = FrameRef {
        ptr: 1024,
        len: exports_frame.bytes().len() as u32,
    };

    let wasm_bytes = wat_parse_str(format!(
        r#"(module
            (import "lisp.env" "call" (func $host_call (param i32 i32 i32 i32) (result i64)))
            (import "lisp.env" "encode" (func $host_encode (param i32 i32) (result i64)))
            (import "lisp.env" "decode" (func $host_decode (param i32 i32) (result i64)))
            (memory (export "memory") 1)
            (global $heap (mut i32) (i32.const 2048))
            (data (i32.const 0) "{}")
            (data (i32.const 1024) "{}")
            (func (export "sim_alloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.tee $ptr
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)
            (func (export "sim_manifest") (result i64)
                i64.const {})
            (func (export "sim_exports") (result i64)
                i64.const {})
            (func (export "sim_call") (param $name_ptr i32) (param $name_len i32) (param $args_ptr i32) (param $args_len i32) (result i64)
                local.get $args_ptr
                local.get $args_len
                call $host_decode
                drop
                local.get $args_ptr
                local.get $args_len
                call $host_encode
                drop
                local.get $name_ptr
                local.get $name_len
                local.get $args_ptr
                local.get $args_len
                call $host_call)
        )"#,
        wat_bytes(manifest_frame.bytes()),
        wat_bytes(exports_frame.bytes()),
        pack_frame_ref(manifest_ref),
        pack_frame_ref(exports_ref),
    ))
    .unwrap();

    let runtime = Arc::new(WasmiRuntime::new());
    let lib = load_wasm_lib_from_bytes(runtime, &wasm_bytes).unwrap();
    let mut cx = wasm_test_cx();
    cx.load_lib(&lib).unwrap();
    let arg = cx.factory().string("callback".to_owned()).unwrap();

    let result = cx
        .call_function(&Symbol::new("distance"), sim_kernel::Args::new(vec![arg]))
        .unwrap();

    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        Expr::List(vec![Expr::String("callback".to_owned())])
    );
}

#[test]
fn wasm_file_loader_reports_missing_path() {
    let runtime = Arc::new(InMemoryWasmRuntime::new());
    let missing = std::env::temp_dir().join("sim-no-such-module.wasm");
    let err = load_wasm_lib_from_file(runtime, &missing).err().unwrap();
    assert!(matches!(err, sim_kernel::Error::HostError(_)));
}

#[test]
fn wasmi_runtime_rejects_invalid_wasm_bytes() {
    let runtime = Arc::new(WasmiRuntime::new());
    let err = load_wasm_lib_from_bytes(runtime, b"not wasm")
        .err()
        .unwrap();
    assert!(matches!(err, sim_kernel::Error::HostError(_)));
}

#[test]
fn wasmi_runtime_rejects_missing_required_exports() {
    let wasm_bytes = wat_parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "sim_alloc") (param i32) (result i32) i32.const 0)
        )"#,
    )
    .unwrap();
    let runtime = Arc::new(WasmiRuntime::new());
    let err = load_wasm_lib_from_bytes(runtime, &wasm_bytes)
        .err()
        .unwrap();
    assert!(matches!(err, sim_kernel::Error::HostError(_)));
}

#[test]
fn wasmi_runtime_rejects_bad_manifest_frame_ref() {
    let wasm_bytes = wat_parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "sim_alloc") (param i32) (result i32) i32.const 0)
            (func (export "sim_manifest") (result i64) i64.const -1)
            (func (export "sim_exports") (result i64) i64.const 0)
            (func (export "sim_call") (param i32 i32 i32 i32) (result i64) i64.const 0)
        )"#,
    )
    .unwrap();
    let runtime = Arc::new(WasmiRuntime::new());
    let err = load_wasm_lib_from_bytes(runtime, &wasm_bytes)
        .err()
        .unwrap();
    assert!(matches!(err, sim_kernel::Error::HostError(_)));
}

fn guest_wat_returning_manifest_ref(packed_manifest: u64) -> Vec<u8> {
    let text = format!(
        r#"(module
            (memory (export "memory") 1)
            (func (export "sim_alloc") (param i32) (result i32) i32.const 0)
            (func (export "sim_manifest") (result i64) i64.const {packed_manifest})
            (func (export "sim_exports") (result i64) i64.const 0)
            (func (export "sim_call") (param i32 i32 i32 i32) (result i64) i64.const 0)
        )"#
    );
    wat_parse_str(text).expect("hand-written wat guest should assemble")
}

#[test]
fn wasmi_rejects_frame_length_over_host_limit() {
    let runtime = WasmiRuntime::new();
    let packed = pack_frame_ref(FrameRef {
        ptr: 0,
        len: u32::MAX,
    });
    let bytes = guest_wat_returning_manifest_ref(packed);
    let handle = runtime.instantiate_bytes(&bytes).unwrap();
    let err = runtime.manifest_frame(handle).unwrap_err();
    assert!(
        matches!(&err, sim_kernel::Error::HostError(message)
            if message.contains("exceeds host limit")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn wasmi_rejects_frame_range_outside_guest_memory() {
    let runtime = WasmiRuntime::with_limits(WasmFrameLimits {
        max_frame_bytes: 64 * 1024 * 1024,
        ..WasmFrameLimits::default()
    });
    let packed = pack_frame_ref(FrameRef {
        ptr: 0,
        len: 1024 * 1024,
    });
    let bytes = guest_wat_returning_manifest_ref(packed);
    let handle = runtime.instantiate_bytes(&bytes).unwrap();
    let err = runtime.manifest_frame(handle).unwrap_err();
    assert!(
        matches!(&err, sim_kernel::Error::HostError(message)
            if message.contains("exceeds guest memory size")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn guest_error_value_becomes_runtime_error() {
    let manifest = manifest();
    let manifest_frame = encode_manifest_frame(&manifest).unwrap();
    let exports_frame = encode_exports_frame(&manifest.exports).unwrap();
    let error_frame = encode_value_frame(&AbiValue::Error("guest boom".to_owned())).unwrap();

    let manifest_ref = FrameRef {
        ptr: 0,
        len: manifest_frame.bytes().len() as u32,
    };
    let exports_ref = FrameRef {
        ptr: 1024,
        len: exports_frame.bytes().len() as u32,
    };
    let error_ref = FrameRef {
        ptr: 2048,
        len: error_frame.bytes().len() as u32,
    };

    let wasm_bytes = wat_parse_str(format!(
        r#"(module
            (memory (export "memory") 1)
            (global $heap (mut i32) (i32.const 4096))
            (data (i32.const 0) "{}")
            (data (i32.const 1024) "{}")
            (data (i32.const 2048) "{}")
            (func (export "sim_alloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.tee $ptr
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)
            (func (export "sim_manifest") (result i64)
                i64.const {})
            (func (export "sim_exports") (result i64)
                i64.const {})
            (func (export "sim_call") (param i32 i32 i32 i32) (result i64)
                i64.const {})
        )"#,
        wat_bytes(manifest_frame.bytes()),
        wat_bytes(exports_frame.bytes()),
        wat_bytes(error_frame.bytes()),
        pack_frame_ref(manifest_ref),
        pack_frame_ref(exports_ref),
        pack_frame_ref(error_ref),
    ))
    .unwrap();

    let runtime = Arc::new(WasmiRuntime::new());
    let lib = load_wasm_lib_from_bytes(runtime, &wasm_bytes).unwrap();
    let mut cx = wasm_test_cx();
    cx.load_lib(&lib).unwrap();
    let err = cx
        .call_function(&Symbol::new("distance"), sim_kernel::Args::new(Vec::new()))
        .unwrap_err();
    assert!(matches!(err, sim_kernel::Error::Eval(message) if message == "guest boom"));
}

#[test]
fn malformed_guest_return_frame_is_rejected() {
    let manifest = manifest();
    let manifest_frame = encode_manifest_frame(&manifest).unwrap();
    let exports_frame = encode_exports_frame(&manifest.exports).unwrap();
    let malformed = b"BAD!".to_vec();

    let manifest_ref = FrameRef {
        ptr: 0,
        len: manifest_frame.bytes().len() as u32,
    };
    let exports_ref = FrameRef {
        ptr: 1024,
        len: exports_frame.bytes().len() as u32,
    };
    let malformed_ref = FrameRef {
        ptr: 2048,
        len: malformed.len() as u32,
    };

    let wasm_bytes = wat_parse_str(format!(
        r#"(module
            (memory (export "memory") 1)
            (global $heap (mut i32) (i32.const 4096))
            (data (i32.const 0) "{}")
            (data (i32.const 1024) "{}")
            (data (i32.const 2048) "{}")
            (func (export "sim_alloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.tee $ptr
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)
            (func (export "sim_manifest") (result i64)
                i64.const {})
            (func (export "sim_exports") (result i64)
                i64.const {})
            (func (export "sim_call") (param i32 i32 i32 i32) (result i64)
                i64.const {})
        )"#,
        wat_bytes(manifest_frame.bytes()),
        wat_bytes(exports_frame.bytes()),
        wat_bytes(&malformed),
        pack_frame_ref(manifest_ref),
        pack_frame_ref(exports_ref),
        pack_frame_ref(malformed_ref),
    ))
    .unwrap();

    let runtime = Arc::new(WasmiRuntime::new());
    let lib = load_wasm_lib_from_bytes(runtime, &wasm_bytes).unwrap();
    let mut cx = wasm_test_cx();
    cx.load_lib(&lib).unwrap();
    let err = cx
        .call_function(&Symbol::new("distance"), sim_kernel::Args::new(Vec::new()))
        .unwrap_err();
    assert!(matches!(err, sim_kernel::Error::HostError(_)));
}

#[test]
fn wasmi_runtime_bounds_infinite_loop_guest_with_fuel() {
    // A guest whose `sim_manifest` loops forever must terminate with an
    // out-of-fuel trap instead of hanging the host thread indefinitely.
    let wasm_bytes = wat_parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "sim_alloc") (param i32) (result i32) i32.const 0)
            (func (export "sim_manifest") (result i64)
                (loop $l (br $l))
                unreachable)
            (func (export "sim_exports") (result i64) i64.const 0)
            (func (export "sim_call") (param i32 i32 i32 i32) (result i64) i64.const 0)
        )"#,
    )
    .unwrap();

    let runtime = WasmiRuntime::with_limits(WasmFrameLimits {
        fuel_per_call: 100_000,
        ..WasmFrameLimits::default()
    });
    let handle = runtime.instantiate_bytes(&wasm_bytes).unwrap();
    let err = runtime.manifest_frame(handle).unwrap_err();
    match err {
        sim_kernel::Error::HostError(message) => {
            assert!(
                message.contains("fuel"),
                "expected an out-of-fuel trap, got: {message}"
            );
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn wasmi_runtime_bounds_memory_growing_guest() {
    // A guest that tries to grow memory far past the configured cap must trap
    // (trap_on_grow_failure) rather than driving the host toward OOM.
    let wasm_bytes = wat_parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "sim_alloc") (param i32) (result i32) i32.const 0)
            (func (export "sim_manifest") (result i64)
                i32.const 1000
                memory.grow
                drop
                i64.const 0)
            (func (export "sim_exports") (result i64) i64.const 0)
            (func (export "sim_call") (param i32 i32 i32 i32) (result i64) i64.const 0)
        )"#,
    )
    .unwrap();

    // 1000 pages (~64 MiB) is within the wasm32 maximum but far past this cap,
    // so the limiter denies the grow and (with trap_on_grow_failure) traps.
    let runtime = WasmiRuntime::with_limits(WasmFrameLimits {
        max_memory_bytes: 2 * 1024 * 1024,
        ..WasmFrameLimits::default()
    });
    let handle = runtime.instantiate_bytes(&wasm_bytes).unwrap();
    let err = runtime.manifest_frame(handle).unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::HostError(ref message) if message.contains("wasmi error")),
        "expected a memory-growth trap, got: {err:?}"
    );
}

fn wat_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("\\{:02x}", byte))
        .collect::<Vec<_>>()
        .join("")
}
