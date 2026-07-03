//! Model plugin ABI records and runtime helpers for wasm guests.

use sim_codec_binary::{decode_frame, encode_frame};
use sim_kernel::{Error, Expr, Result};
use wasmi::{
    Config, Engine, Instance, Linker as WasmiLinker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

use crate::Frame;

const PAGE_SIZE: usize = 65_536;
const EXPORT_ALLOC: &str = "sim_alloc";

/// Required guest export the host calls for one model inference.
pub const EXPORT_MODEL_INFER: &str = "sim_model_infer";
/// Required guest export the host calls for the backend's model card.
pub const EXPORT_MODEL_CARD: &str = "sim_model_card";
/// Linear memory export read by the host.
pub const EXPORT_MEMORY: &str = "memory";

/// A `(pointer, length)` model frame inside guest linear memory.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmModelFrameRef {
    /// Byte offset of the frame within guest linear memory.
    pub ptr: u32,
    /// Byte length of the frame.
    pub len: u32,
}

/// Host limits for the model wasm runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmModelAbiLimits {
    /// Fuel budget granted to each infer or card call.
    pub fuel_per_infer: u64,
    /// Maximum guest memory size in wasm pages.
    pub max_memory_pages: u32,
    /// Maximum byte length of one model request or response frame.
    pub max_frame_bytes: usize,
}

impl Default for WasmModelAbiLimits {
    fn default() -> Self {
        Self {
            fuel_per_infer: 5_000_000_000,
            max_memory_pages: 4096,
            max_frame_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Encodes a model request or response expression as a binary model frame.
pub fn encode_model_expr_frame(expr: &Expr) -> Result<Frame> {
    Ok(Frame::new(encode_frame(expr)?.0))
}

/// Decodes a binary model frame into its expression payload.
pub fn decode_model_expr_frame(frame: &Frame) -> Result<Expr> {
    let (_, expr) = decode_frame(sim_kernel::CodecId(0), frame.bytes())?;
    Ok(expr)
}

/// Wasmi-backed model plugin instance.
pub struct WasmModelInstance {
    store: Store<StoreLimits>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    model_card: TypedFunc<(), u64>,
    model_infer: TypedFunc<(u32, u32), u64>,
    limits: WasmModelAbiLimits,
}

impl WasmModelInstance {
    /// Instantiates a model guest module from wasm bytes under `limits`.
    pub fn from_bytes_with_limits(bytes: &[u8], limits: WasmModelAbiLimits) -> Result<Self> {
        let engine = fuel_engine();
        let module = Module::new(&engine, bytes).map_err(wasmi_error)?;
        let max_memory_pages = usize::try_from(limits.max_memory_pages)
            .map_err(|_| Error::HostError("wasm model memory limit overflows usize".to_owned()))?;
        let max_memory_bytes = max_memory_pages.checked_mul(PAGE_SIZE).ok_or_else(|| {
            Error::HostError("wasm model memory limit overflows usize".to_owned())
        })?;
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(max_memory_bytes)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(&engine, store_limits);
        store.limiter(|limits| limits);
        store.set_fuel(limits.fuel_per_infer).map_err(wasmi_error)?;
        let linker = WasmiLinker::new(&engine);
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(wasmi_error)?;
        let memory = instance
            .get_export(&store, EXPORT_MEMORY)
            .and_then(|export| export.into_memory())
            .ok_or_else(|| Error::HostError("wasm model missing exported memory".to_owned()))?;
        let alloc = typed_func::<u32, u32>(&store, &instance, EXPORT_ALLOC)?;
        let model_card = typed_func::<(), u64>(&store, &instance, EXPORT_MODEL_CARD)?;
        let model_infer = typed_func::<(u32, u32), u64>(&store, &instance, EXPORT_MODEL_INFER)?;
        Ok(Self {
            store,
            memory,
            alloc,
            model_card,
            model_infer,
            limits,
        })
    }

    /// Reads the model card frame reported by the guest.
    pub fn model_card_frame(&mut self) -> Result<Frame> {
        self.refuel()?;
        let packed = self
            .model_card
            .call(&mut self.store, ())
            .map_err(wasmi_error)?;
        self.read_guest_frame(packed)
    }

    /// Runs one model inference call through the guest.
    pub fn infer_frame(&mut self, request: Frame) -> Result<Frame> {
        self.refuel()?;
        let (ptr, len) = self.write_guest_bytes(request.bytes())?;
        let packed = self
            .model_infer
            .call(&mut self.store, (ptr, len))
            .map_err(wasmi_error)?;
        self.read_guest_frame(packed)
    }

    fn refuel(&mut self) -> Result<()> {
        self.store
            .set_fuel(self.limits.fuel_per_infer)
            .map_err(wasmi_error)
    }

    fn write_guest_bytes(&mut self, bytes: &[u8]) -> Result<(u32, u32)> {
        let len = u32::try_from(bytes.len())
            .map_err(|_| Error::HostError("wasm model frame exceeds u32 length".to_owned()))?;
        if bytes.len() > self.limits.max_frame_bytes {
            return Err(Error::HostError(format!(
                "wasm model frame length {} exceeds host limit {} bytes",
                bytes.len(),
                self.limits.max_frame_bytes
            )));
        }
        let ptr = self.alloc.call(&mut self.store, len).map_err(wasmi_error)?;
        self.memory
            .write(&mut self.store, ptr as usize, bytes)
            .map_err(wasmi_error)?;
        Ok((ptr, len))
    }

    fn read_guest_frame(&mut self, packed: u64) -> Result<Frame> {
        let frame = unpack_model_frame_ref(packed);
        let len = frame.len as usize;
        if len > self.limits.max_frame_bytes {
            return Err(Error::HostError(format!(
                "wasm model frame length {len} exceeds host limit {} bytes",
                self.limits.max_frame_bytes
            )));
        }
        let ptr = frame.ptr as usize;
        let end = ptr
            .checked_add(len)
            .ok_or_else(|| Error::HostError("wasm model frame range overflows usize".to_owned()))?;
        let memory_size = self.memory.data_size(&self.store);
        if end > memory_size {
            return Err(Error::HostError(format!(
                "wasm model frame range {ptr}..{end} exceeds guest memory size {memory_size}"
            )));
        }
        let mut bytes = vec![0u8; len];
        self.memory
            .read(&self.store, ptr, &mut bytes)
            .map_err(wasmi_error)?;
        Ok(Frame::new(bytes))
    }
}

/// Packs a model frame reference into the `i64`/`u64` ABI return value.
pub fn pack_model_frame_ref(frame: WasmModelFrameRef) -> u64 {
    ((frame.len as u64) << 32) | frame.ptr as u64
}

/// Unpacks a model frame reference from the ABI return value.
pub fn unpack_model_frame_ref(packed: u64) -> WasmModelFrameRef {
    WasmModelFrameRef {
        ptr: (packed & 0xffff_ffff) as u32,
        len: (packed >> 32) as u32,
    }
}

fn fuel_engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    Engine::new(&config)
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

fn wasmi_error(err: impl core::fmt::Display) -> Error {
    Error::HostError(format!("wasmi model error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_model_frame_layout() {
        assert_eq!(std::mem::size_of::<WasmModelFrameRef>(), 8);
        let frame = WasmModelFrameRef {
            ptr: 0x0102_0304,
            len: 0x0506_0708,
        };
        assert_eq!(unpack_model_frame_ref(pack_model_frame_ref(frame)), frame);
    }

    #[test]
    fn model_expr_frame_round_trips() {
        let expr = Expr::String("model-frame".to_owned());
        let frame = encode_model_expr_frame(&expr).unwrap();
        assert_eq!(decode_model_expr_frame(&frame).unwrap(), expr);
    }
}
