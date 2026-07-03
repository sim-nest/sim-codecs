use crate::{
    AbiValue, Frame, Handle, WasmDependency, WasmExport, WasmGuestModule, WasmHost, WasmManifest,
    WasmRuntime, decode_value_frame, encode_exports_frame, encode_manifest_frame,
    encode_value_frame, register_stub_exports,
};
use sim_kernel::{
    Cx, DefaultFactory, EagerPolicy, Expr, Lib, LibManifest, LibTarget, Result, Symbol,
    read_construct_capability,
};
use std::sync::Arc;

pub(super) struct StubWasmLib {
    pub(super) manifest: WasmManifest,
}

impl Lib for StubWasmLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.to_lib_manifest()
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut sim_kernel::Linker<'_>) -> Result<()> {
        register_stub_exports(cx, linker, &self.manifest.exports)
    }
}

pub(super) struct FakeHost {
    pub(super) manifest: WasmManifest,
    pub(super) expected_module_bytes: Vec<u8>,
}

impl WasmHost for FakeHost {
    fn manifest_frame(&self, _module: Handle) -> Result<Frame> {
        encode_manifest_frame(&self.manifest)
    }

    fn exports_frame(&self, _module: Handle) -> Result<Frame> {
        encode_exports_frame(&self.manifest.exports)
    }

    fn call(&self, _module: Handle, function: &Symbol, args: Frame) -> Result<Frame> {
        let AbiValue::Expr(Expr::List(args)) = decode_value_frame(&args)? else {
            return Err(sim_kernel::Error::TypeMismatch {
                expected: "expr list args",
                found: "non-list args",
            });
        };
        let response = match function.to_string().as_str() {
            "distance" => AbiValue::Expr(Expr::Number(sim_kernel::NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: args.len().to_string(),
            })),
            "point" => AbiValue::Expr(Expr::Map(vec![
                (Expr::Symbol(Symbol::new("x")), args[0].clone()),
                (Expr::Symbol(Symbol::new("y")), args[1].clone()),
            ])),
            other => AbiValue::Error(format!("unknown guest function {other}")),
        };
        encode_value_frame(&response)
    }
}

impl WasmRuntime for FakeHost {
    fn instantiate_bytes(&self, bytes: &[u8]) -> Result<Handle> {
        if bytes == self.expected_module_bytes.as_slice() {
            Ok(Handle(7))
        } else {
            Err(sim_kernel::Error::HostError(
                "unexpected wasm module bytes".to_owned(),
            ))
        }
    }
}

pub(super) struct FakeGuestModule {
    pub(super) manifest: WasmManifest,
}

impl WasmGuestModule for FakeGuestModule {
    fn manifest_frame(&self) -> Result<Frame> {
        encode_manifest_frame(&self.manifest)
    }

    fn exports_frame(&self) -> Result<Frame> {
        encode_exports_frame(&self.manifest.exports)
    }

    fn call(&self, function: &Symbol, args: Frame) -> Result<Frame> {
        let AbiValue::Expr(Expr::List(args)) = decode_value_frame(&args)? else {
            return Err(sim_kernel::Error::TypeMismatch {
                expected: "expr list args",
                found: "non-list args",
            });
        };
        let response = match function.to_string().as_str() {
            "distance" => AbiValue::Expr(Expr::Number(sim_kernel::NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: args.len().to_string(),
            })),
            "point" => AbiValue::Expr(Expr::Map(vec![
                (Expr::Symbol(Symbol::new("x")), args[0].clone()),
                (Expr::Symbol(Symbol::new("y")), args[1].clone()),
            ])),
            other => AbiValue::Error(format!("unknown guest function {other}")),
        };
        encode_value_frame(&response)
    }
}

pub(super) fn manifest() -> WasmManifest {
    WasmManifest {
        id: Symbol::new("geometry"),
        version: "0.1.0".to_owned(),
        abi: sim_kernel::AbiVersion { major: 0, minor: 1 },
        target: LibTarget::WasmComponent,
        requires: vec![WasmDependency {
            id: Symbol::qualified("codec", "binary"),
            minimum_version: Some("0.1.0".to_owned()),
        }],
        capabilities: vec!["read-construct".to_owned()],
        exports: vec![
            WasmExport::Class {
                symbol: Symbol::new("Point"),
                constructor: Some(Symbol::new("point")),
            },
            WasmExport::Function {
                symbol: Symbol::new("point"),
            },
            WasmExport::Function {
                symbol: Symbol::new("distance"),
            },
        ],
    }
}

pub(super) fn manifest_with_codec_export() -> WasmManifest {
    let mut manifest = manifest();
    manifest.exports.push(WasmExport::Codec {
        symbol: Symbol::qualified("codec", "guest"),
    });
    manifest
}

pub(super) fn wasm_test_cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    cx.grant(read_construct_capability());
    cx
}
