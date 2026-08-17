//! Lossless, index-preserving JVM constant-pool grammar.

use core::fmt;

use sim_text::CodeUnitString;

use crate::{ByteError, ByteReader, ByteWriter, decode_modified_utf8, encode_modified_utf8};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("constant/model.rs");
include!("constant/codec.rs");
