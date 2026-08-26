//! Pure v2 bundle encoding for complete SIM Index vault projections.
//!
//! ```
//! use sim_codec_index_vault::{resolve_profile, VaultEncoder};
//! let encoder = VaultEncoder::new(resolve_profile("portable")?);
//! # let _ = encoder;
//! # Ok::<(), sim_codec_index_vault::VaultCodecError>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use sha2::{Digest, Sha256};
use sim_codec_doc::{
    AttributeEnvelope, DialectMarkdownBackend, Inline, LinkDialect, MarkdownDialect, MarkupBackend,
    MarkupBlock, MarkupDecodeOptions, MarkupDoc, MarkupEncodeOptions,
};
use sim_codec_index::{
    IndexForm, decode_index_expr, encode_index_expr, expr_from_index_doc, index_doc_from_expr,
};
use sim_index_core::IndexDoc;
use sim_index_vault_core::{
    IndexRow, VaultGranularity, VaultNoteKind, VaultNotePlan, VaultProjection,
};
use sim_kernel::{ContentId, Expr, Symbol};

const MAX_NOTES: usize = 50_000;
const MAX_ROWS: usize = 100_000;
const MAX_NOTE_BYTES: usize = 1024 * 1024;
const MAX_BUNDLE_BYTES: usize = 128 * 1024 * 1024;
const ROOT: &str = "SIM-Index";

mod codec;
mod error;
mod helpers;
mod profiles;

pub use codec::*;
pub use error::*;
pub use profiles::*;

use codec::row_family;
use helpers::*;
