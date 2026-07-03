//! Audio plugin ABI records and export/import names for wasm guests.

use sim_kernel::{Error, Result};

const HEADER_SIZE: usize = 8;
const NAME_LEN: usize = 64;
const VENDOR_LEN: usize = 32;
const STABLE_ID_LEN: usize = 64;

/// Required wasm export returning the audio manifest pointer.
pub const EXPORT_MANIFEST_PTR: &str = "sim_audio_manifest_ptr";
/// Required wasm export preparing the plugin for a sample rate and block size.
pub const EXPORT_PREPARE: &str = "sim_audio_prepare";
/// Required wasm export resetting plugin state.
pub const EXPORT_RESET: &str = "sim_audio_reset";
/// Required wasm export processing one host-provided audio block.
pub const EXPORT_PROCESS: &str = "sim_audio_process";

/// Required wasm import module for host callbacks.
pub const IMPORT_MODULE: &str = "env";
/// Required wasm import returning the current host block frame count.
pub const IMPORT_FRAME_COUNT: &str = "host_frame_count";
/// Required wasm import reading one input sample.
pub const IMPORT_AUDIO_READ: &str = "host_audio_read";
/// Required wasm import writing one output sample.
pub const IMPORT_AUDIO_WRITE: &str = "host_audio_write";
/// Required wasm import reading one host parameter value.
pub const IMPORT_PARAM_GET: &str = "host_param_get";

/// Flat manifest written by a wasm audio guest to linear memory.
///
/// The host reads [`WasmAudioManifest::SIZE`] bytes from the pointer returned by
/// [`EXPORT_MANIFEST_PTR`]. Integer fields are little-endian in memory, matching
/// the wasm target. String fields are UTF-8 text, null-padded to their declared
/// capacities.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmAudioManifest {
    /// Number of input audio channels.
    pub audio_in_channels: u16,
    /// Number of output audio channels.
    pub audio_out_channels: u16,
    /// Number of plain floating-point parameters.
    pub param_count: u16,
    /// Reserved padding field.
    pub _pad: u16,
    /// Plugin display name, UTF-8, null-padded.
    pub name: [u8; NAME_LEN],
    /// Plugin vendor, UTF-8, null-padded.
    pub vendor: [u8; VENDOR_LEN],
    /// Backend-stable plugin id, UTF-8, null-padded.
    pub stable_id: [u8; STABLE_ID_LEN],
}

impl WasmAudioManifest {
    /// Encoded manifest byte length.
    pub const SIZE: usize = HEADER_SIZE + NAME_LEN + VENDOR_LEN + STABLE_ID_LEN;

    /// Decodes a manifest from its byte representation.
    ///
    /// # Errors
    ///
    /// Returns an error when `bytes` is shorter than [`Self::SIZE`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes = bytes
            .get(..Self::SIZE)
            .ok_or_else(|| Error::Eval("wasm audio manifest is truncated".to_owned()))?;
        let audio_in_channels = read_u16(bytes, 0);
        let audio_out_channels = read_u16(bytes, 2);
        let param_count = read_u16(bytes, 4);
        let pad = read_u16(bytes, 6);
        let mut name = [0; NAME_LEN];
        name.copy_from_slice(&bytes[HEADER_SIZE..HEADER_SIZE + NAME_LEN]);
        let vendor_start = HEADER_SIZE + NAME_LEN;
        let mut vendor = [0; VENDOR_LEN];
        vendor.copy_from_slice(&bytes[vendor_start..vendor_start + VENDOR_LEN]);
        let stable_id_start = vendor_start + VENDOR_LEN;
        let mut stable_id = [0; STABLE_ID_LEN];
        stable_id.copy_from_slice(&bytes[stable_id_start..stable_id_start + STABLE_ID_LEN]);
        Ok(Self {
            audio_in_channels,
            audio_out_channels,
            param_count,
            _pad: pad,
            name,
            vendor,
            stable_id,
        })
    }

    /// Returns the plugin name, or `unknown` when the field is not valid UTF-8.
    pub fn name_str(&self) -> &str {
        utf8_null_padded(&self.name).unwrap_or("unknown")
    }

    /// Returns the plugin vendor, or an empty string when the field is invalid.
    pub fn vendor_str(&self) -> &str {
        utf8_null_padded(&self.vendor).unwrap_or("")
    }

    /// Returns the stable plugin id, or an empty string when the field is invalid.
    pub fn stable_id_str(&self) -> &str {
        utf8_null_padded(&self.stable_id).unwrap_or("")
    }
}

/// Guest-side behavior trait used by the `sim_audio_impl!` macro.
pub trait SimAudioGuest {
    /// Prepares the plugin for host audio processing.
    fn prepare(sample_rate_hz: f64, max_block_frames: u32);

    /// Resets plugin state.
    fn reset();

    /// Processes the current host-provided audio block.
    fn process() -> i32;
}

/// Exports the mandatory SIM audio plugin ABI functions for a guest type.
#[macro_export]
macro_rules! sim_audio_impl {
    ($plugin:ty, $manifest:expr) => {
        static SIM_AUDIO_MANIFEST: $crate::audio::WasmAudioManifest = $manifest;

        #[unsafe(no_mangle)]
        pub extern "C" fn sim_audio_manifest_ptr() -> u32 {
            core::ptr::addr_of!(SIM_AUDIO_MANIFEST) as u32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn sim_audio_prepare(sample_rate_hz: f64, max_block_frames: u32) {
            <$plugin as $crate::audio::SimAudioGuest>::prepare(sample_rate_hz, max_block_frames);
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn sim_audio_reset() {
            <$plugin as $crate::audio::SimAudioGuest>::reset();
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn sim_audio_process() -> i32 {
            <$plugin as $crate::audio::SimAudioGuest>::process()
        }
    };
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn utf8_null_padded(bytes: &[u8]) -> Option<&str> {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_audio_manifest_size_is_168() {
        assert_eq!(WasmAudioManifest::SIZE, 168);
        assert_eq!(std::mem::size_of::<WasmAudioManifest>(), 168);
    }

    #[test]
    fn wasm_audio_manifest_header_offset_matches_wat_data() {
        let mut bytes = vec![0; WasmAudioManifest::SIZE];
        bytes[0..8].copy_from_slice(&[2, 0, 2, 0, 1, 0, 0, 0]);
        bytes[8..12].copy_from_slice(b"gain");
        bytes[72..75].copy_from_slice(b"sim");
        bytes[104..112].copy_from_slice(b"sim.gain");

        let manifest = WasmAudioManifest::from_bytes(&bytes).expect("manifest");
        assert_eq!(manifest.audio_in_channels, 2);
        assert_eq!(manifest.audio_out_channels, 2);
        assert_eq!(manifest.param_count, 1);
        assert_eq!(manifest.name_str(), "gain");
        assert_eq!(manifest.vendor_str(), "sim");
        assert_eq!(manifest.stable_id_str(), "sim.gain");
    }
}
