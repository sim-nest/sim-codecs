//! Backend registry and fidelity contracts for markup codecs.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use sim_kernel::CodecId;

use crate::markdown::MarkdownBackend;
use crate::markup::{BackendId, MarkupDoc};
use crate::typst_backend::TypstBackend;

/// Decode options shared by markup backends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupDecodeOptions {
    /// Preserve backend source text when the backend can do so.
    pub preserve_source: bool,
    /// Preserve backend-specific raw fragments when possible.
    pub preserve_raw: bool,
}

impl Default for MarkupDecodeOptions {
    fn default() -> Self {
        Self {
            preserve_source: true,
            preserve_raw: true,
        }
    }
}

/// Encode options shared by markup backends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupEncodeOptions {
    /// Treat any reported loss as an encode error.
    pub fail_on_loss: bool,
    /// Preserve backend-specific raw nodes when possible.
    pub preserve_raw: bool,
}

impl Default for MarkupEncodeOptions {
    fn default() -> Self {
        Self {
            fail_on_loss: true,
            preserve_raw: true,
        }
    }
}

/// A single lossy conversion note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupLoss {
    /// Stable path to the affected document part.
    pub path: String,
    /// Human-readable loss reason.
    pub reason: String,
}

/// Fidelity report returned by markup backends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupFidelity {
    /// Backend that produced the report.
    pub backend: BackendId,
    /// Raw backend fragments preserved in the semantic document.
    pub preserved_raw: Vec<String>,
    /// Semantic parts dropped during conversion.
    pub dropped: Vec<MarkupLoss>,
    /// Non-fatal warnings, such as ambiguous source constructs.
    pub warnings: Vec<String>,
}

impl MarkupFidelity {
    /// Create an exact, warning-free report for `backend`.
    pub fn exact(backend: BackendId) -> Self {
        Self {
            backend,
            preserved_raw: Vec::new(),
            dropped: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Error returned by markup backend and registry operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkupError {
    /// No backend is registered for the requested id.
    UnknownBackend(BackendId),
    /// Backend decoding failed.
    Decode(String),
    /// Backend encoding failed.
    Encode(String),
    /// The input expression is not a markup document value.
    InvalidDocument(String),
}

impl MarkupError {
    pub(crate) fn into_kernel_error(self, codec: CodecId) -> sim_kernel::Error {
        sim_kernel::Error::CodecError {
            codec,
            message: self.to_string(),
        }
    }
}

impl fmt::Display for MarkupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownBackend(id) => write!(f, "unknown markup backend {id}"),
            Self::Decode(message) => write!(f, "markup decode failed: {message}"),
            Self::Encode(message) => write!(f, "markup encode failed: {message}"),
            Self::InvalidDocument(message) => write!(f, "invalid markup document: {message}"),
        }
    }
}

impl StdError for MarkupError {}

/// A concrete markup reader/writer behind a runtime codec id.
pub trait MarkupBackend: Send + Sync {
    /// Stable backend id, such as `markdown`, `typst`, `asciidoc`, or `latex`.
    fn id(&self) -> BackendId;

    /// Decode backend source text into the shared markup document IR.
    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError>;

    /// Encode the shared markup document IR into backend source text.
    fn encode(
        &self,
        doc: &MarkupDoc,
        opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError>;
}

/// Deterministic registry of markup backends.
#[derive(Clone, Default)]
pub struct BackendRegistry {
    backends: BTreeMap<BackendId, Arc<dyn MarkupBackend>>,
}

impl BackendRegistry {
    /// Create an empty backend registry.
    pub fn new() -> Self {
        Self {
            backends: BTreeMap::new(),
        }
    }

    /// Register `backend`, returning any backend previously registered with
    /// the same id.
    pub fn register<B: MarkupBackend + 'static>(
        &mut self,
        backend: B,
    ) -> Option<Arc<dyn MarkupBackend>> {
        self.register_arc(Arc::new(backend))
    }

    /// Register an already shared backend handle.
    pub fn register_arc(
        &mut self,
        backend: Arc<dyn MarkupBackend>,
    ) -> Option<Arc<dyn MarkupBackend>> {
        self.backends.insert(backend.id(), backend)
    }

    /// Return a backend handle by id.
    pub fn backend(&self, id: &BackendId) -> Result<Arc<dyn MarkupBackend>, MarkupError> {
        self.backends
            .get(id)
            .cloned()
            .ok_or_else(|| MarkupError::UnknownBackend(id.clone()))
    }

    /// Return backend ids in deterministic registry order.
    pub fn ids(&self) -> Vec<BackendId> {
        self.backends.keys().cloned().collect()
    }

    /// Iterate over backends in deterministic registry order.
    pub fn iter(&self) -> impl Iterator<Item = (&BackendId, &Arc<dyn MarkupBackend>)> {
        self.backends.iter()
    }

    /// Whether this registry contains no backends.
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

/// Compatibility name for the default Markdown backend.
#[derive(Clone, Debug, Default)]
pub struct BasicMarkdownBackend;

impl MarkupBackend for BasicMarkdownBackend {
    fn id(&self) -> BackendId {
        BackendId::new("markdown")
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        MarkdownBackend.decode(input, opts)
    }

    fn encode(
        &self,
        doc: &MarkupDoc,
        opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        MarkdownBackend.encode(doc, opts)
    }
}

/// Build the default registry installed by [`install_doc_codec`](crate::install_doc_codec).
pub fn default_backend_registry() -> BackendRegistry {
    let mut registry = BackendRegistry::new();
    registry.register(MarkdownBackend);
    registry.register(TypstBackend);
    registry
}
