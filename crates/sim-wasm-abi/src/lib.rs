//! Wasm ABI transport for SIM.
//!
//! Defines the byte-frame and manifest transport between the host and a wasm
//! guest: the frame codec that encodes/decodes value, manifest, and export
//! frames, the host/runtime traits, and the `WasmLib` that surfaces a guest
//! module's exports as a kernel lib. This crate carries only ABI transport, not
//! guest semantics.
//!
//! Module map (all modules are private; the public surface is re-exported from
//! this crate root):
//! - codec: encode/decode of value, manifest, and export byte frames.
//! - library: `WasmLib` and the loaders that instantiate a guest module as a
//!   kernel lib (plus stub-export registration).
//! - runtime: the in-memory test runtime and the wasmi-backed runtime
//!   implementing the host/runtime traits.
//! - types: the ABI value, frame, handle, manifest, export, and host/runtime
//!   trait definitions.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(feature = "audio")]
pub mod audio;
mod codec;
mod library;
#[cfg(feature = "model")]
pub mod model;
mod runtime;
mod types;

pub use codec::{
    decode_exports_frame, decode_manifest_frame, decode_value_frame, encode_exports_frame,
    encode_manifest_frame, encode_value_frame,
};
pub use library::{
    WasmLib, load_wasm_lib_from_bytes, load_wasm_lib_from_file, register_stub_exports,
};
pub use runtime::{InMemoryWasmRuntime, WasmiRuntime};
pub use types::{
    AbiValue, Frame, FrameRef, Handle, WasmDependency, WasmExport, WasmFrameLimits,
    WasmGuestModule, WasmHost, WasmManifest, WasmRuntime,
};

#[cfg(test)]
mod tests;
