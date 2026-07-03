//! `WasmLib`: surfaces an instantiated guest module as a kernel lib by reading
//! its manifest and export frames, plus the loaders that build one from wasm
//! bytes or a file and the stub-export registration helper.

use crate::codec::{
    decode_exports_frame, decode_manifest_frame, decode_value_frame, encode_value_frame,
};
use crate::types::{AbiValue, Handle, WasmExport, WasmHost, WasmRuntime};
use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Lib, LibManifest, Linker, LoadCx, Object, Result,
    Symbol, Value,
};
use std::path::Path;
use std::sync::Arc;

/// A kernel [`Lib`] backed by an instantiated guest module.
///
/// Reads the module's manifest and export frames once at construction, then
/// binds each guest export when loaded: functions and sites become callable
/// proxies that round-trip through the host, values are bound directly, and
/// export kinds the v1 ABI cannot execute are recorded as unsupported.
pub struct WasmLib {
    module: Handle,
    host: Arc<dyn WasmHost>,
    manifest_cache: LibManifest,
    exports_cache: Vec<WasmExport>,
}

impl WasmLib {
    /// Builds a `WasmLib` over a module already instantiated on `host`.
    ///
    /// Decodes and caches the module's manifest and export frames; errors if
    /// either frame is malformed.
    pub fn instantiate(host: Arc<dyn WasmHost>, module: Handle) -> Result<Self> {
        let manifest = decode_manifest_frame(&host.manifest_frame(module)?)
            .map_err(|err| Error::HostError(format!("invalid wasm manifest frame: {err}")))?;
        let exports = decode_exports_frame(&host.exports_frame(module)?)
            .map_err(|err| Error::HostError(format!("invalid wasm exports frame: {err}")))?;
        Ok(Self {
            module,
            host,
            manifest_cache: manifest.to_lib_manifest(),
            exports_cache: exports,
        })
    }
}

/// Instantiates `bytes` on `runtime` and surfaces the module as a [`WasmLib`].
pub fn load_wasm_lib_from_bytes(runtime: Arc<dyn WasmRuntime>, bytes: &[u8]) -> Result<WasmLib> {
    let module = runtime.instantiate_bytes(bytes)?;
    let host: Arc<dyn WasmHost> = runtime;
    WasmLib::instantiate(host, module)
}

/// Reads a wasm module from `path` and loads it as a [`WasmLib`] on `runtime`.
///
/// Errors if the file cannot be read.
pub fn load_wasm_lib_from_file(
    runtime: Arc<dyn WasmRuntime>,
    path: impl AsRef<Path>,
) -> Result<WasmLib> {
    let bytes = std::fs::read(path.as_ref()).map_err(|err| {
        Error::HostError(format!(
            "failed to read wasm module {}: {err}",
            path.as_ref().display()
        ))
    })?;
    load_wasm_lib_from_bytes(runtime, &bytes)
}

impl Lib for WasmLib {
    fn manifest(&self) -> LibManifest {
        self.manifest_cache.clone()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        for export in &self.exports_cache {
            match export {
                WasmExport::Function { symbol } => {
                    linker.function_value(
                        symbol.clone(),
                        cx.factory()
                            .opaque(Arc::new(WasmFunction::new(
                                self.host.clone(),
                                self.module,
                                symbol.clone(),
                            )))
                            .expect("wasm function should be boxable"),
                    )?;
                }
                WasmExport::Macro { symbol } => {
                    linker.unsupported_export(
                        sim_kernel::ExportKind::named(sim_kernel::ExportKind::MACRO),
                        symbol.clone(),
                        "wasm ABI v1 cannot execute macro runtime exports",
                    )?;
                }
                WasmExport::Class {
                    symbol,
                    constructor: _,
                } => {
                    linker.unsupported_export(
                        sim_kernel::ExportKind::named(sim_kernel::ExportKind::CLASS),
                        symbol.clone(),
                        "wasm ABI v1 cannot execute class runtime exports",
                    )?;
                }
                WasmExport::Codec { symbol } => {
                    linker.unsupported_export(
                        sim_kernel::ExportKind::named(sim_kernel::ExportKind::CODEC),
                        symbol.clone(),
                        "wasm ABI v1 cannot execute codec runtime exports",
                    )?;
                }
                WasmExport::Shape { symbol } => {
                    linker.unsupported_export(
                        sim_kernel::ExportKind::named(sim_kernel::ExportKind::SHAPE),
                        symbol.clone(),
                        "wasm ABI v1 cannot execute shape runtime exports",
                    )?;
                }
                WasmExport::NumberDomain { symbol } => {
                    linker.unsupported_export(
                        sim_kernel::ExportKind::named(sim_kernel::ExportKind::NUMBER_DOMAIN),
                        symbol.clone(),
                        "wasm ABI v1 cannot execute number-domain runtime exports",
                    )?;
                }
                WasmExport::Site { symbol } => {
                    linker.site_value(
                        symbol.clone(),
                        cx.factory()
                            .opaque(Arc::new(WasmSite::new(
                                self.host.clone(),
                                self.module,
                                symbol.clone(),
                            )))
                            .expect("wasm site should be boxable"),
                    )?;
                }
                WasmExport::Value { symbol } => {
                    linker.value(symbol.clone(), cx.factory().symbol(symbol.clone())?)?;
                }
            }
        }
        Ok(())
    }
}

/// Binds `exports` into `linker` without a live guest, for host-only tests.
///
/// Function and site exports become non-executable stub callables and other
/// kinds are recorded as unsupported, matching how a real [`WasmLib`] would
/// load them.
pub fn register_stub_exports(
    cx: &mut LoadCx,
    linker: &mut Linker<'_>,
    exports: &[WasmExport],
) -> Result<()> {
    for export in exports {
        match export {
            WasmExport::Function { symbol } => {
                linker.function_value(
                    symbol.clone(),
                    cx.factory()
                        .opaque(Arc::new(WasmFunctionStub::new(symbol.clone())))
                        .expect("wasm function stub should be boxable"),
                )?;
            }
            WasmExport::Macro { symbol } => {
                linker.unsupported_export(
                    sim_kernel::ExportKind::named(sim_kernel::ExportKind::MACRO),
                    symbol.clone(),
                    "wasm ABI v1 cannot execute macro runtime exports",
                )?;
            }
            WasmExport::Class {
                symbol,
                constructor: _,
            } => {
                linker.unsupported_export(
                    sim_kernel::ExportKind::named(sim_kernel::ExportKind::CLASS),
                    symbol.clone(),
                    "wasm ABI v1 cannot execute class runtime exports",
                )?;
            }
            WasmExport::Codec { symbol } => {
                linker.unsupported_export(
                    sim_kernel::ExportKind::named(sim_kernel::ExportKind::CODEC),
                    symbol.clone(),
                    "wasm ABI v1 cannot execute codec runtime exports",
                )?;
            }
            WasmExport::Shape { symbol } => {
                linker.unsupported_export(
                    sim_kernel::ExportKind::named(sim_kernel::ExportKind::SHAPE),
                    symbol.clone(),
                    "wasm ABI v1 cannot execute shape runtime exports",
                )?;
            }
            WasmExport::NumberDomain { symbol } => {
                linker.unsupported_export(
                    sim_kernel::ExportKind::named(sim_kernel::ExportKind::NUMBER_DOMAIN),
                    symbol.clone(),
                    "wasm ABI v1 cannot execute number-domain runtime exports",
                )?;
            }
            WasmExport::Site { symbol } => {
                linker.site_value(
                    symbol.clone(),
                    cx.factory()
                        .opaque(Arc::new(WasmSiteStub::new(symbol.clone())))
                        .expect("wasm site stub should be boxable"),
                )?;
            }
            WasmExport::Value { symbol } => {
                linker.value(symbol.clone(), cx.factory().symbol(symbol.clone())?)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct WasmFunctionStub {
    symbol: Symbol,
}

impl WasmFunctionStub {
    fn new(symbol: Symbol) -> Self {
        Self { symbol }
    }
}

impl Object for WasmFunctionStub {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<wasm-function {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for WasmFunctionStub {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Function"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_FUNCTION_CLASS_ID,
            Symbol::qualified("core", "Function"),
        )
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }
}

impl Callable for WasmFunctionStub {
    fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
        Err(Error::Eval(format!(
            "wasm function stub {} is not executable in host-only tests",
            self.symbol
        )))
    }
}

#[derive(Clone)]
struct WasmSiteStub {
    symbol: Symbol,
}

impl WasmSiteStub {
    fn new(symbol: Symbol) -> Self {
        Self { symbol }
    }
}

impl Object for WasmSiteStub {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<wasm-site {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for WasmSiteStub {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        expr_class(cx)
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }
}

impl Callable for WasmSiteStub {
    fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
        Err(Error::Eval(format!(
            "wasm site stub {} is not executable in host-only tests",
            self.symbol
        )))
    }
}

#[derive(Clone)]
struct WasmFunction {
    host: Arc<dyn WasmHost>,
    module: Handle,
    symbol: Symbol,
}

impl WasmFunction {
    fn new(host: Arc<dyn WasmHost>, module: Handle, symbol: Symbol) -> Self {
        Self {
            host,
            module,
            symbol,
        }
    }
}

impl Object for WasmFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<wasm-function {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for WasmFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Function"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_FUNCTION_CLASS_ID,
            Symbol::qualified("core", "Function"),
        )
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }
}

impl Callable for WasmFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        call_wasm_guest(self.host.as_ref(), self.module, &self.symbol, cx, args)
    }
}

#[derive(Clone)]
struct WasmSite {
    host: Arc<dyn WasmHost>,
    module: Handle,
    symbol: Symbol,
}

impl WasmSite {
    fn new(host: Arc<dyn WasmHost>, module: Handle, symbol: Symbol) -> Self {
        Self {
            host,
            module,
            symbol,
        }
    }

    fn realize_symbol(&self) -> Symbol {
        Symbol::new(format!("{}/realize", self.symbol.as_qualified_str()))
    }
}

impl Object for WasmSite {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<wasm-site {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for WasmSite {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        expr_class(cx)
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }
}

impl Callable for WasmSite {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        call_wasm_guest(
            self.host.as_ref(),
            self.module,
            &self.realize_symbol(),
            cx,
            args,
        )
    }
}

fn call_wasm_guest(
    host: &dyn WasmHost,
    module: Handle,
    symbol: &Symbol,
    cx: &mut Cx,
    args: Args,
) -> Result<Value> {
    let expr_args = args
        .into_vec()
        .into_iter()
        .map(|value| value.object().as_expr(cx))
        .collect::<Result<Vec<_>>>()?;
    let frame = encode_value_frame(&AbiValue::Expr(Expr::List(expr_args)))?;
    let returned = host.call(module, symbol, frame)?;
    let value = decode_value_frame(&returned)
        .map_err(|err| Error::HostError(format!("invalid wasm return frame: {err}")))?;
    abi_value_to_runtime_value(cx, value)
}

fn expr_class(cx: &mut Cx) -> Result<ClassRef> {
    let symbol = Symbol::qualified("core", "Expr");
    if let Some(value) = cx.registry().class_by_symbol(&symbol) {
        return Ok(value.clone());
    }
    cx.factory()
        .class_stub(sim_kernel::CORE_EXPR_CLASS_ID, symbol)
}

fn abi_value_to_runtime_value(cx: &mut Cx, value: AbiValue) -> Result<Value> {
    match value {
        AbiValue::Expr(expr) => expr_to_runtime_value(cx, expr),
        AbiValue::Handle(handle) => cx.factory().expr(Expr::Extension {
            tag: Symbol::qualified("wasm", "handle"),
            payload: Box::new(Expr::String(handle.0.to_string())),
        }),
        AbiValue::Error(message) => Err(Error::Eval(message)),
    }
}

fn expr_to_runtime_value(cx: &mut Cx, expr: Expr) -> Result<Value> {
    match expr {
        Expr::Nil => cx.factory().nil(),
        Expr::Bool(value) => cx.factory().bool(value),
        Expr::Number(number) => cx.factory().number_literal(number.domain, number.canonical),
        Expr::Symbol(symbol) => cx.factory().symbol(symbol),
        Expr::String(value) => cx.factory().string(value),
        Expr::Bytes(value) => cx.factory().bytes(value),
        Expr::List(items) => {
            let values = items
                .into_iter()
                .map(|item| expr_to_runtime_value(cx, item))
                .collect::<Result<Vec<_>>>()?;
            cx.factory().list(values)
        }
        Expr::Map(entries) => {
            let table = entries
                .into_iter()
                .map(|(key, value)| {
                    let Expr::Symbol(symbol) = key else {
                        return Err(Error::TypeMismatch {
                            expected: "symbol key",
                            found: "non-symbol key",
                        });
                    };
                    Ok((symbol, expr_to_runtime_value(cx, value)?))
                })
                .collect::<Result<Vec<_>>>()?;
            cx.factory().table(table)
        }
        other => cx.factory().expr(other),
    }
}
