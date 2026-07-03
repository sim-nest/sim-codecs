use super::support::{FakeHost, StubWasmLib, manifest, manifest_with_codec_export, wasm_test_cx};
use crate::{
    AbiValue, Frame, Handle, WasmExport, WasmHost, WasmLib, WasmManifest, WasmRuntime,
    decode_exports_frame, decode_manifest_frame, decode_value_frame, encode_exports_frame,
    encode_manifest_frame, encode_value_frame, load_wasm_lib_from_bytes, load_wasm_lib_from_file,
};
use sim_kernel::{Args, Export, ExportState, Expr, Lib, LibTarget, Symbol};
use std::sync::Arc;

#[test]
fn abi_values_roundtrip_through_binary_frames() {
    let expr_value = AbiValue::Expr(sim_kernel::Expr::String("ok".to_owned()));
    assert_eq!(
        decode_value_frame(&encode_value_frame(&expr_value).unwrap()).unwrap(),
        expr_value
    );

    let handle_value = AbiValue::Handle(Handle(42));
    assert_eq!(
        decode_value_frame(&encode_value_frame(&handle_value).unwrap()).unwrap(),
        handle_value
    );

    let error_value = AbiValue::Error("boom".to_owned());
    assert_eq!(
        decode_value_frame(&encode_value_frame(&error_value).unwrap()).unwrap(),
        error_value
    );
}

#[test]
fn manifest_roundtrips_through_binary_frame_boundary() {
    let manifest = manifest();
    let decoded = decode_manifest_frame(&encode_manifest_frame(&manifest).unwrap()).unwrap();
    assert_eq!(decoded, manifest);
}

#[test]
fn exports_roundtrip_through_binary_frame_boundary() {
    let exports = manifest().exports;
    let decoded = decode_exports_frame(&encode_exports_frame(&exports).unwrap()).unwrap();
    assert_eq!(decoded, exports);
}

#[test]
fn wasm_manifest_converts_to_and_from_lib_manifest() {
    let wasm = manifest();
    let lib = wasm.to_lib_manifest();
    let converted = crate::WasmManifest::from_lib_manifest(&lib);
    assert_eq!(converted.id, wasm.id);
    assert_eq!(converted.version, wasm.version);
    assert_eq!(converted.abi, wasm.abi);
    assert_eq!(converted.target, wasm.target);
    assert_eq!(converted.requires, wasm.requires);
    assert_eq!(converted.capabilities, wasm.capabilities);
    assert!(converted
        .exports
        .iter()
        .any(|export| matches!(export, WasmExport::Class { symbol, .. } if symbol == &Symbol::new("Point"))));
    assert!(converted
        .exports
        .iter()
        .any(|export| matches!(export, WasmExport::Function { symbol } if symbol == &Symbol::new("point"))));
}

#[test]
fn site_export_roundtrips_through_wasm_manifest_transport() {
    let mut wasm = manifest();
    let site = Symbol::qualified("model", "loaded-site");
    wasm.exports.push(WasmExport::Site {
        symbol: site.clone(),
    });

    let decoded = decode_manifest_frame(&encode_manifest_frame(&wasm).unwrap()).unwrap();
    assert!(
        decoded
            .exports
            .iter()
            .any(|export| matches!(export, WasmExport::Site { symbol } if symbol == &site))
    );

    let lib = decoded.to_lib_manifest();
    assert!(lib.exports.iter().any(
        |export| matches!(export, Export::Site { symbol, runtime_id: None } if symbol == &site)
    ));

    let converted = crate::WasmManifest::from_lib_manifest(&lib);
    assert!(
        converted
            .exports
            .iter()
            .any(|export| matches!(export, WasmExport::Site { symbol } if symbol == &site))
    );
}

#[test]
fn stub_wasm_lib_registers_functions_and_records_class_as_unsupported() {
    let mut cx = wasm_test_cx();
    let lib = StubWasmLib {
        manifest: manifest(),
    };
    cx.load_lib(&lib).unwrap();
    assert!(cx.resolve_function(&Symbol::new("point")).is_ok());
    assert!(cx.resolve_function(&Symbol::new("distance")).is_ok());
    let loaded = cx.registry().lib(&Symbol::new("geometry")).unwrap();
    let export = loaded
        .exports
        .iter()
        .find(|export| export.symbol == Symbol::new("Point"))
        .unwrap();
    assert!(matches!(
        &export.state,
        ExportState::Unsupported { reason }
            if reason.contains("class runtime exports")
    ));
}

#[test]
fn wasm_codec_exports_are_reported_as_unsupported_until_implemented() {
    let mut cx = wasm_test_cx();
    let lib = StubWasmLib {
        manifest: manifest_with_codec_export(),
    };
    cx.load_lib(&lib).unwrap();
    let loaded = cx.registry().lib(&Symbol::new("geometry")).unwrap();
    let export = loaded
        .exports
        .iter()
        .find(|export| export.symbol == Symbol::qualified("codec", "guest"))
        .unwrap();
    assert!(matches!(
        &export.state,
        ExportState::Unsupported { reason }
            if reason.contains("codec runtime exports")
    ));
}

#[test]
fn wasm_macro_exports_are_reported_as_unsupported_until_implemented() {
    let mut manifest = manifest();
    manifest.exports.push(WasmExport::Macro {
        symbol: Symbol::qualified("macro", "guest"),
    });
    let mut cx = wasm_test_cx();
    let lib = StubWasmLib { manifest };
    cx.load_lib(&lib).unwrap();
    let loaded = cx.registry().lib(&Symbol::new("geometry")).unwrap();
    let export = loaded
        .exports
        .iter()
        .find(|export| export.symbol == Symbol::qualified("macro", "guest"))
        .unwrap();
    assert!(matches!(
        &export.state,
        ExportState::Unsupported { reason }
            if reason.contains("macro runtime exports")
    ));
}

#[test]
fn frame_ref_reports_frame_length() {
    let frame = Frame::new(vec![1, 2, 3, 4]);
    assert_eq!(frame.as_ref().unwrap().len, 4);
}

#[test]
fn wasm_lib_instantiates_from_host_and_calls_guest_functions() {
    let host = Arc::new(FakeHost {
        manifest: manifest(),
        expected_module_bytes: b"(fake wasm module)".to_vec(),
    });
    let lib = WasmLib::instantiate(host, Handle(7)).unwrap();
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
fn wasm_site_export_registers_callable_site_value_and_forwards_realize() {
    let site_symbol = Symbol::qualified("model", "wasm-site");
    let mut manifest = manifest();
    manifest.exports.push(WasmExport::Site {
        symbol: site_symbol.clone(),
    });
    let host = Arc::new(SiteHost {
        manifest,
        expected_module_bytes: b"(fake wasm site module)".to_vec(),
        site_symbol: site_symbol.clone(),
    });
    let lib = load_wasm_lib_from_bytes(host, b"(fake wasm site module)").unwrap();
    let mut cx = wasm_test_cx();
    cx.load_lib(&lib).unwrap();

    let arg = cx.factory().string("site request".to_owned()).unwrap();
    let site = cx.registry().site_by_symbol(&site_symbol).unwrap().clone();
    assert_eq!(
        site.object().as_expr(&mut cx).unwrap(),
        Expr::Symbol(site_symbol)
    );
    let reply = site
        .object()
        .as_callable()
        .unwrap()
        .call(&mut cx, Args::new(vec![arg]))
        .unwrap();
    assert_eq!(
        reply.object().as_expr(&mut cx).unwrap(),
        Expr::String("wasm site answer".to_owned())
    );
}

#[test]
fn wasm_lib_loads_from_runtime_bytes_boundary() {
    let runtime = Arc::new(FakeHost {
        manifest: manifest(),
        expected_module_bytes: b"(fake wasm module)".to_vec(),
    });
    let lib = load_wasm_lib_from_bytes(runtime, b"(fake wasm module)").unwrap();
    assert_eq!(lib.manifest().id, Symbol::new("geometry"));
    assert_eq!(lib.manifest().target, LibTarget::WasmComponent);
}

struct SiteHost {
    manifest: WasmManifest,
    expected_module_bytes: Vec<u8>,
    site_symbol: Symbol,
}

impl WasmHost for SiteHost {
    fn manifest_frame(&self, _module: Handle) -> sim_kernel::Result<Frame> {
        encode_manifest_frame(&self.manifest)
    }

    fn exports_frame(&self, _module: Handle) -> sim_kernel::Result<Frame> {
        encode_exports_frame(&self.manifest.exports)
    }

    fn call(&self, _module: Handle, function: &Symbol, args: Frame) -> sim_kernel::Result<Frame> {
        assert_eq!(
            function.to_string(),
            format!("{}/realize", self.site_symbol)
        );
        let AbiValue::Expr(Expr::List(args)) = decode_value_frame(&args)? else {
            return Err(sim_kernel::Error::TypeMismatch {
                expected: "expr list args",
                found: "non-list args",
            });
        };
        assert!(matches!(
            args.as_slice(),
            [Expr::String(text)] if text == "site request"
        ));
        encode_value_frame(&AbiValue::Expr(Expr::String("wasm site answer".to_owned())))
    }
}

impl WasmRuntime for SiteHost {
    fn instantiate_bytes(&self, bytes: &[u8]) -> sim_kernel::Result<Handle> {
        if bytes == self.expected_module_bytes.as_slice() {
            Ok(Handle(19))
        } else {
            Err(sim_kernel::Error::HostError(
                "unexpected wasm site module bytes".to_owned(),
            ))
        }
    }
}

#[test]
fn wasm_lib_loads_from_file_boundary() {
    let runtime = Arc::new(FakeHost {
        manifest: manifest(),
        expected_module_bytes: b"(fake wasm module file)".to_vec(),
    });
    let dir = std::env::temp_dir();
    let path = dir.join("sim-fake-module.wasm");
    std::fs::write(&path, b"(fake wasm module file)").unwrap();
    let lib = load_wasm_lib_from_file(runtime, &path).unwrap();
    assert_eq!(lib.manifest().id, Symbol::new("geometry"));
    std::fs::remove_file(&path).unwrap();
}
