//! Core ABI transport types: the `AbiValue` payload, `Frame`/`FrameRef`,
//! `Handle`, manifest and export records, frame limits, and the
//! `WasmHost`/`WasmRuntime`/`WasmGuestModule` traits the runtimes implement.

use sim_kernel::{
    AbiVersion, Dependency, Error, Export, Expr, LibManifest, LibTarget, Result, Symbol, Version,
};

/// Opaque host-side identifier for an instantiated guest module.
///
/// A runtime mints one `Handle` per module it instantiates and uses it to route
/// later manifest, export, and call requests back to that module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handle(pub u64);

/// A `(pointer, length)` view of a byte frame inside guest linear memory.
///
/// The host packs this into a single `u64` on the ABI boundary and reads the
/// referenced bytes out of guest memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRef {
    /// Byte offset of the frame within guest linear memory.
    pub ptr: u32,
    /// Byte length of the frame.
    pub len: u32,
}

/// Host limits applied to a guest module: the frame ceiling plus the CPU and
/// memory bounds that keep an untrusted guest from hanging or OOMing the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmFrameLimits {
    /// Maximum byte length a single guest frame may report.
    /// Kept compatible with the binary codec frame ceiling on purpose.
    pub max_frame_bytes: usize,
    /// Fuel budget granted to the guest for each host-driven call (manifest,
    /// exports, and function invocations). Fuel is roughly one unit per executed
    /// wasm instruction; a guest that exhausts it (an infinite loop, say) traps
    /// instead of hanging the host thread. The store is refueled to this value
    /// before every call.
    pub fuel_per_call: u64,
    /// Maximum size, in bytes, a guest linear memory may grow to. A guest that
    /// tries to grow past this traps rather than driving the host to OOM.
    pub max_memory_bytes: usize,
    /// Maximum number of elements a guest table may grow to.
    pub max_table_elements: usize,
}

impl Default for WasmFrameLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 8 * 1024 * 1024,
            // Generous for real plugin work, finite for hostile loops.
            fuel_per_call: 1_000_000_000,
            // 64 MiB: well above the 8 MiB frame ceiling, far below host OOM.
            max_memory_bytes: 64 * 1024 * 1024,
            max_table_elements: 100_000,
        }
    }
}

/// An owned ABI byte frame: the encoded payload exchanged across the boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    bytes: Vec<u8>,
}

impl Frame {
    /// Wraps already-encoded bytes as a frame.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Returns an empty frame carrying no bytes.
    pub fn empty() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Borrows the frame's encoded bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the frame and returns its owned bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Returns a host-relative `FrameRef` for the frame.
    ///
    /// Errors if the frame is longer than `u32::MAX` bytes.
    pub fn as_ref(&self) -> Result<FrameRef> {
        Ok(FrameRef {
            ptr: 0,
            len: u32::try_from(self.bytes.len())
                .map_err(|_| Error::HostError("frame exceeds u32 length".to_owned()))?,
        })
    }
}

/// A decoded ABI value carried by a value frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiValue {
    /// An ordinary expression value.
    Expr(Expr),
    /// An opaque guest-module handle.
    Handle(Handle),
    /// A guest-reported error message.
    Error(String),
}

/// A dependency a guest module declares in its manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmDependency {
    /// Symbol of the required lib.
    pub id: Symbol,
    /// Minimum acceptable version, if the guest pins one.
    pub minimum_version: Option<String>,
}

/// A single export a guest module surfaces through its manifest/export frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WasmExport {
    /// A callable function export.
    Function {
        /// Symbol the function is bound under.
        symbol: Symbol,
    },
    /// A macro export.
    Macro {
        /// Symbol the macro is bound under.
        symbol: Symbol,
    },
    /// A class export.
    Class {
        /// Symbol the class is bound under.
        symbol: Symbol,
        /// Optional constructor function symbol.
        constructor: Option<Symbol>,
    },
    /// A codec export.
    Codec {
        /// Symbol the codec is bound under.
        symbol: Symbol,
    },
    /// A shape export.
    Shape {
        /// Symbol the shape is bound under.
        symbol: Symbol,
    },
    /// A number-domain export.
    NumberDomain {
        /// Symbol the number domain is bound under.
        symbol: Symbol,
    },
    /// An opaque site export.
    Site {
        /// Symbol the site is bound under.
        symbol: Symbol,
    },
    /// A plain value export.
    Value {
        /// Symbol the value is bound under.
        symbol: Symbol,
    },
}

/// The ABI form of a lib manifest as carried in a manifest frame.
///
/// Mirrors the kernel's lib manifest in transport-friendly types; convert with
/// [`WasmManifest::from_lib_manifest`] and [`WasmManifest::to_lib_manifest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmManifest {
    /// Symbol identifying the lib.
    pub id: Symbol,
    /// Lib version string.
    pub version: String,
    /// ABI version the guest was built against.
    pub abi: AbiVersion,
    /// Lib target kind.
    pub target: LibTarget,
    /// Declared dependencies.
    pub requires: Vec<WasmDependency>,
    /// Declared capability names.
    pub capabilities: Vec<String>,
    /// Exports the guest surfaces.
    pub exports: Vec<WasmExport>,
}

/// Host interface to an already-instantiated guest module.
///
/// Implemented by the runtimes; consumed by `WasmLib` to read a module's
/// manifest and export frames and to invoke its functions.
pub trait WasmHost: Send + Sync {
    /// Returns the module's manifest frame.
    fn manifest_frame(&self, module: Handle) -> Result<Frame>;
    /// Returns the module's export frame.
    fn exports_frame(&self, module: Handle) -> Result<Frame>;
    /// Invokes `function` on the module with the argument frame and returns the
    /// result frame.
    fn call(&self, module: Handle, function: &Symbol, args: Frame) -> Result<Frame>;
}

/// A [`WasmHost`] that can also instantiate new modules from wasm bytes.
pub trait WasmRuntime: WasmHost {
    /// Instantiates a module from wasm bytes and returns its handle.
    fn instantiate_bytes(&self, bytes: &[u8]) -> Result<Handle>;
}

/// A single guest module's host-facing surface, addressed without a handle.
///
/// Used by the in-memory runtime to back a registered module with a concrete
/// implementation.
pub trait WasmGuestModule: Send + Sync {
    /// Returns this module's manifest frame.
    fn manifest_frame(&self) -> Result<Frame>;
    /// Returns this module's export frame.
    fn exports_frame(&self) -> Result<Frame>;
    /// Invokes `function` with the argument frame and returns the result frame.
    fn call(&self, function: &Symbol, args: Frame) -> Result<Frame>;
}

impl WasmManifest {
    /// Builds a transport manifest from a kernel [`LibManifest`].
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_wasm_abi::WasmManifest;
    /// use sim_kernel::{AbiVersion, LibTarget, Symbol};
    ///
    /// let wasm = WasmManifest {
    ///     id: Symbol::new("geometry"),
    ///     version: "0.1.0".to_owned(),
    ///     abi: AbiVersion { major: 0, minor: 1 },
    ///     target: LibTarget::WasmComponent,
    ///     requires: Vec::new(),
    ///     capabilities: Vec::new(),
    ///     exports: Vec::new(),
    /// };
    /// let lib = wasm.to_lib_manifest();
    /// assert_eq!(WasmManifest::from_lib_manifest(&lib).id, wasm.id);
    /// ```
    pub fn from_lib_manifest(manifest: &LibManifest) -> Self {
        Self {
            id: manifest.id.clone(),
            version: manifest.version.0.clone(),
            abi: manifest.abi,
            target: manifest.target.clone(),
            requires: manifest
                .requires
                .iter()
                .map(|dependency| WasmDependency {
                    id: dependency.id.clone(),
                    minimum_version: dependency.minimum_version.as_ref().map(|v| v.0.clone()),
                })
                .collect(),
            capabilities: manifest
                .capabilities
                .iter()
                .map(|capability| capability.as_str().to_owned())
                .collect(),
            exports: manifest
                .exports
                .iter()
                .map(WasmExport::from_export)
                .collect(),
        }
    }

    /// Converts the transport manifest back into a kernel [`LibManifest`].
    pub fn to_lib_manifest(&self) -> LibManifest {
        LibManifest {
            id: self.id.clone(),
            version: Version(self.version.clone()),
            abi: self.abi,
            target: self.target.clone(),
            requires: self
                .requires
                .iter()
                .map(|dependency| Dependency {
                    id: dependency.id.clone(),
                    minimum_version: dependency.minimum_version.clone().map(Version),
                })
                .collect(),
            capabilities: self
                .capabilities
                .iter()
                .map(|capability| sim_kernel::CapabilityName::new(capability.clone()))
                .collect(),
            exports: self.exports.iter().map(WasmExport::to_export).collect(),
        }
    }
}

impl WasmExport {
    /// Builds a transport export from a kernel [`Export`], dropping runtime ids.
    pub fn from_export(export: &Export) -> Self {
        match export {
            Export::Class { symbol, .. } => Self::Class {
                symbol: symbol.clone(),
                constructor: None,
            },
            Export::Function { symbol, .. } => Self::Function {
                symbol: symbol.clone(),
            },
            Export::Macro { symbol, .. } => Self::Macro {
                symbol: symbol.clone(),
            },
            Export::Shape { symbol, .. } => Self::Shape {
                symbol: symbol.clone(),
            },
            Export::Codec { symbol, .. } => Self::Codec {
                symbol: symbol.clone(),
            },
            Export::NumberDomain { symbol, .. } => Self::NumberDomain {
                symbol: symbol.clone(),
            },
            Export::Site { symbol, .. } => Self::Site {
                symbol: symbol.clone(),
            },
            Export::Value { symbol } => Self::Value {
                symbol: symbol.clone(),
            },
            #[allow(unreachable_patterns)]
            export => panic!(
                "wasm ABI v1 cannot encode open export declaration kind {} for symbol {}",
                export.kind_symbol().symbol(),
                export.symbol()
            ),
        }
    }

    /// Converts the transport export back into a kernel [`Export`] with no ids.
    pub fn to_export(&self) -> Export {
        match self {
            Self::Function { symbol } => Export::Function {
                symbol: symbol.clone(),
                function_id: None,
            },
            Self::Macro { symbol } => Export::Macro {
                symbol: symbol.clone(),
                macro_id: None,
            },
            Self::Class { symbol, .. } => Export::Class {
                symbol: symbol.clone(),
                class_id: None,
            },
            Self::Codec { symbol } => Export::Codec {
                symbol: symbol.clone(),
                codec_id: None,
            },
            Self::Shape { symbol } => Export::Shape {
                symbol: symbol.clone(),
                shape_id: None,
            },
            Self::NumberDomain { symbol } => Export::NumberDomain {
                symbol: symbol.clone(),
                number_domain_id: None,
            },
            Self::Site { symbol } => Export::Site {
                symbol: symbol.clone(),
                runtime_id: None,
            },
            Self::Value { symbol } => Export::Value {
                symbol: symbol.clone(),
            },
        }
    }
}
