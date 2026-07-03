//! Wasm runtime implementations of the host/runtime traits: an in-memory
//! runtime backed by registered guest-module stubs for tests, and a
//! wasmi-backed runtime that instantiates real wasm modules.

use crate::types::{
    Frame, FrameRef, Handle, WasmFrameLimits, WasmGuestModule, WasmHost, WasmRuntime,
};
use sim_kernel::{Error, Result, Symbol};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use wasmi::{
    Caller, Config, Engine, Instance, Linker as WasmiLinker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

const WASM_MEMORY_EXPORT: &str = "memory";
// These guest export names intentionally use the post-rename sim_* ABI.
// This is a guest ABI break from the old pre-rename names.
const WASM_ALLOC_EXPORT: &str = "sim_alloc";
const WASM_MANIFEST_EXPORT: &str = "sim_manifest";
const WASM_EXPORTS_EXPORT: &str = "sim_exports";
const WASM_CALL_EXPORT: &str = "sim_call";
const HOST_IMPORT_MODULE: &str = "lisp.env";

/// A test runtime that serves pre-registered guest-module stubs.
///
/// Modules are registered under their wasm bytes and instantiated by matching
/// those exact bytes, so tests can exercise the loader and host traits without
/// a real wasm engine.
#[derive(Default)]
pub struct InMemoryWasmRuntime {
    state: Mutex<InMemoryWasmRuntimeState>,
}

#[derive(Default)]
struct InMemoryWasmRuntimeState {
    next_handle: u64,
    registered: BTreeMap<Vec<u8>, Arc<dyn WasmGuestModule>>,
    instantiated: BTreeMap<Handle, Arc<dyn WasmGuestModule>>,
}

impl InMemoryWasmRuntime {
    /// Creates an empty in-memory runtime with no registered modules.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `module` under the given wasm `bytes`.
    ///
    /// Errors if a module is already registered under identical bytes.
    pub fn register_module(
        &self,
        bytes: impl Into<Vec<u8>>,
        module: Arc<dyn WasmGuestModule>,
    ) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("in-memory wasm runtime mutex poisoned".to_owned()))?;
        let bytes = bytes.into();
        if state.registered.contains_key(&bytes) {
            return Err(Error::HostError(
                "duplicate in-memory wasm module bytes".to_owned(),
            ));
        }
        state.registered.insert(bytes, module);
        Ok(())
    }
}

impl WasmHost for InMemoryWasmRuntime {
    fn manifest_frame(&self, module: Handle) -> Result<Frame> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("in-memory wasm runtime mutex poisoned".to_owned()))?;
        let module = state
            .instantiated
            .get(&module)
            .ok_or_else(|| Error::HostError(format!("unknown wasm module handle {}", module.0)))?;
        module.manifest_frame()
    }

    fn exports_frame(&self, module: Handle) -> Result<Frame> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("in-memory wasm runtime mutex poisoned".to_owned()))?;
        let module = state
            .instantiated
            .get(&module)
            .ok_or_else(|| Error::HostError(format!("unknown wasm module handle {}", module.0)))?;
        module.exports_frame()
    }

    fn call(&self, module: Handle, function: &Symbol, args: Frame) -> Result<Frame> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("in-memory wasm runtime mutex poisoned".to_owned()))?;
        let module = state
            .instantiated
            .get(&module)
            .ok_or_else(|| Error::HostError(format!("unknown wasm module handle {}", module.0)))?;
        module.call(function, args)
    }
}

impl WasmRuntime for InMemoryWasmRuntime {
    fn instantiate_bytes(&self, bytes: &[u8]) -> Result<Handle> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("in-memory wasm runtime mutex poisoned".to_owned()))?;
        let module =
            state.registered.get(bytes).cloned().ok_or_else(|| {
                Error::HostError("unknown in-memory wasm module bytes".to_owned())
            })?;
        state.next_handle += 1;
        let handle = Handle(state.next_handle);
        state.instantiated.insert(handle, module);
        Ok(handle)
    }
}

/// A wasmi-backed runtime that instantiates and drives real wasm modules.
///
/// Implements the host/runtime traits by calling the guest's `sim_*` ABI
/// exports, reading frames out of guest linear memory under [`WasmFrameLimits`],
/// and exposing the minimal `lisp.env` host imports.
pub struct WasmiRuntime {
    engine: Engine,
    limits: WasmFrameLimits,
    state: Mutex<WasmiRuntimeState>,
}

#[derive(Default)]
struct WasmiRuntimeState {
    next_handle: u64,
    modules: BTreeMap<Handle, WasmiModuleInstance>,
}

struct WasmiModuleInstance {
    store: Store<StoreLimits>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    manifest: TypedFunc<(), u64>,
    exports: TypedFunc<(), u64>,
    call: TypedFunc<(u32, u32, u32, u32), u64>,
}

impl Default for WasmiRuntime {
    fn default() -> Self {
        Self::with_limits(WasmFrameLimits::default())
    }
}

impl WasmiRuntime {
    /// Creates a wasmi runtime with default frame limits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a wasmi runtime that applies the given frame limits.
    pub fn with_limits(limits: WasmFrameLimits) -> Self {
        Self {
            engine: build_fuel_engine(),
            limits,
            state: Mutex::new(WasmiRuntimeState::default()),
        }
    }
}

/// Builds the wasmi engine with fuel metering enabled so guest CPU can be
/// bounded per call. Memory/table growth is bounded separately by a
/// [`StoreLimits`] limiter installed on each store.
fn build_fuel_engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    Engine::new(&config)
}

impl WasmHost for WasmiRuntime {
    fn manifest_frame(&self, module: Handle) -> Result<Frame> {
        let limits = self.limits;
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("wasmi runtime mutex poisoned".to_owned()))?;
        let module = state
            .modules
            .get_mut(&module)
            .ok_or_else(|| Error::HostError(format!("unknown wasmi module handle {}", module.0)))?;
        refuel(module, limits)?;
        let packed = module
            .manifest
            .call(&mut module.store, ())
            .map_err(wasmi_error)?;
        read_guest_frame(module, packed, limits)
    }

    fn exports_frame(&self, module: Handle) -> Result<Frame> {
        let limits = self.limits;
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("wasmi runtime mutex poisoned".to_owned()))?;
        let module = state
            .modules
            .get_mut(&module)
            .ok_or_else(|| Error::HostError(format!("unknown wasmi module handle {}", module.0)))?;
        refuel(module, limits)?;
        let packed = module
            .exports
            .call(&mut module.store, ())
            .map_err(wasmi_error)?;
        read_guest_frame(module, packed, limits)
    }

    fn call(&self, module: Handle, function: &Symbol, args: Frame) -> Result<Frame> {
        let limits = self.limits;
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("wasmi runtime mutex poisoned".to_owned()))?;
        let module = state
            .modules
            .get_mut(&module)
            .ok_or_else(|| Error::HostError(format!("unknown wasmi module handle {}", module.0)))?;
        refuel(module, limits)?;
        let function_bytes = function.to_string().into_bytes();
        let (name_ptr, name_len) = write_guest_bytes(module, &function_bytes)?;
        let (args_ptr, args_len) = write_guest_bytes(module, args.bytes())?;
        let packed = module
            .call
            .call(&mut module.store, (name_ptr, name_len, args_ptr, args_len))
            .map_err(wasmi_error)?;
        read_guest_frame(module, packed, limits)
    }
}

impl WasmRuntime for WasmiRuntime {
    fn instantiate_bytes(&self, bytes: &[u8]) -> Result<Handle> {
        let module = Module::new(&self.engine, bytes).map_err(wasmi_error)?;
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.max_memory_bytes)
            .table_elements(self.limits.max_table_elements)
            // Make a denied grow trap rather than returning -1, so a guest that
            // tries to balloon memory fails closed instead of driving host OOM.
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(&self.engine, store_limits);
        store.limiter(|limits| limits);
        // Fuel must be charged before the start function runs, or instantiation
        // itself traps with out-of-fuel.
        store
            .set_fuel(self.limits.fuel_per_call)
            .map_err(wasmi_error)?;
        let mut linker = WasmiLinker::new(&self.engine);
        define_host_imports(&mut linker)?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(wasmi_error)?;
        let memory = instance
            .get_export(&store, WASM_MEMORY_EXPORT)
            .and_then(|extern_| extern_.into_memory())
            .ok_or_else(|| Error::HostError("wasm module missing exported memory".to_owned()))?;
        let alloc = typed_func::<u32, u32>(&store, &instance, WASM_ALLOC_EXPORT)?;
        let manifest = typed_func::<(), u64>(&store, &instance, WASM_MANIFEST_EXPORT)?;
        let exports = typed_func::<(), u64>(&store, &instance, WASM_EXPORTS_EXPORT)?;
        let call = typed_func::<(u32, u32, u32, u32), u64>(&store, &instance, WASM_CALL_EXPORT)?;

        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::HostError("wasmi runtime mutex poisoned".to_owned()))?;
        state.next_handle += 1;
        let handle = Handle(state.next_handle);
        state.modules.insert(
            handle,
            WasmiModuleInstance {
                store,
                memory,
                alloc,
                manifest,
                exports,
                call,
            },
        );
        Ok(handle)
    }
}

fn define_host_imports(linker: &mut WasmiLinker<StoreLimits>) -> Result<()> {
    linker
        .func_wrap(
            HOST_IMPORT_MODULE,
            "call",
            |caller: Caller<'_, StoreLimits>,
             name_ptr: i32,
             name_len: i32,
             args_ptr: i32,
             args_len: i32|
             -> std::result::Result<i64, wasmi::Error> {
                validate_guest_frame(&caller, name_ptr, name_len)?;
                let frame = validate_guest_frame(&caller, args_ptr, args_len)?;
                Ok(pack_frame_ref(frame) as i64)
            },
        )
        .map_err(wasmi_error)?;
    linker
        .func_wrap(
            HOST_IMPORT_MODULE,
            "encode",
            |caller: Caller<'_, StoreLimits>,
             ptr: i32,
             len: i32|
             -> std::result::Result<i64, wasmi::Error> {
                let frame = validate_guest_frame(&caller, ptr, len)?;
                Ok(pack_frame_ref(frame) as i64)
            },
        )
        .map_err(wasmi_error)?;
    linker
        .func_wrap(
            HOST_IMPORT_MODULE,
            "decode",
            |caller: Caller<'_, StoreLimits>,
             ptr: i32,
             len: i32|
             -> std::result::Result<i64, wasmi::Error> {
                let frame = validate_guest_frame(&caller, ptr, len)?;
                Ok(pack_frame_ref(frame) as i64)
            },
        )
        .map_err(wasmi_error)?;
    Ok(())
}

fn validate_guest_frame(
    caller: &Caller<'_, StoreLimits>,
    ptr: i32,
    len: i32,
) -> std::result::Result<FrameRef, wasmi::Error> {
    let ptr = u32::try_from(ptr)
        .map_err(|_| wasmi::Error::new("host import frame pointer is negative"))?;
    let len = u32::try_from(len)
        .map_err(|_| wasmi::Error::new("host import frame length is negative"))?;
    let memory = caller
        .get_export(WASM_MEMORY_EXPORT)
        .and_then(|extern_| extern_.into_memory())
        .ok_or_else(|| wasmi::Error::new("host import caller has no exported memory"))?;
    let end = (ptr as usize)
        .checked_add(len as usize)
        .ok_or_else(|| wasmi::Error::new("host import frame range overflows usize"))?;
    let memory_size = memory.data_size(caller);
    if end > memory_size {
        return Err(wasmi::Error::new(format!(
            "host import frame range {}..{} exceeds guest memory size {}",
            ptr, end, memory_size
        )));
    }
    Ok(FrameRef { ptr, len })
}

fn typed_func<Params, Results>(
    store: &Store<StoreLimits>,
    instance: &Instance,
    name: &str,
) -> Result<TypedFunc<Params, Results>>
where
    Params: wasmi::WasmParams,
    Results: wasmi::WasmResults,
{
    instance
        .get_typed_func::<Params, Results>(store, name)
        .map_err(wasmi_error)
}

pub(crate) fn pack_frame_ref(frame: FrameRef) -> u64 {
    ((frame.len as u64) << 32) | frame.ptr as u64
}

fn unpack_frame_ref(packed: u64) -> FrameRef {
    FrameRef {
        ptr: (packed & 0xffff_ffff) as u32,
        len: (packed >> 32) as u32,
    }
}

fn read_guest_frame(
    module: &mut WasmiModuleInstance,
    packed: u64,
    limits: WasmFrameLimits,
) -> Result<Frame> {
    let frame = unpack_frame_ref(packed);
    let len = frame.len as usize;
    if len > limits.max_frame_bytes {
        return Err(Error::HostError(format!(
            "wasm guest frame length {len} exceeds host limit {} bytes",
            limits.max_frame_bytes
        )));
    }

    let ptr = frame.ptr as usize;
    let end = ptr
        .checked_add(len)
        .ok_or_else(|| Error::HostError("wasm guest frame range overflows usize".to_owned()))?;
    let memory_size = module.memory.data_size(&module.store);
    if end > memory_size {
        return Err(Error::HostError(format!(
            "wasm guest frame range {ptr}..{end} exceeds guest memory size {memory_size}"
        )));
    }

    let mut bytes = vec![0u8; len];
    module
        .memory
        .read(&module.store, ptr, &mut bytes)
        .map_err(wasmi_error)?;
    Ok(Frame::new(bytes))
}

fn write_guest_bytes(module: &mut WasmiModuleInstance, bytes: &[u8]) -> Result<(u32, u32)> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| Error::HostError("guest write exceeds u32 length".to_owned()))?;
    let ptr = module
        .alloc
        .call(&mut module.store, len)
        .map_err(wasmi_error)?;
    module
        .memory
        .write(&mut module.store, ptr as usize, bytes)
        .map_err(wasmi_error)?;
    Ok((ptr, len))
}

/// Resets the guest's fuel to its per-call budget before a host-driven call so
/// each call gets a fresh CPU allowance and an infinite-loop guest traps with an
/// out-of-fuel error instead of hanging the host thread.
fn refuel(module: &mut WasmiModuleInstance, limits: WasmFrameLimits) -> Result<()> {
    module
        .store
        .set_fuel(limits.fuel_per_call)
        .map_err(wasmi_error)
}

fn wasmi_error(err: impl core::fmt::Display) -> Error {
    Error::HostError(format!("wasmi error: {err}"))
}
