//! Structured JVM attributes whose encoded order and indices are semantically significant.
//!
//! This module validates binary format and local static constraints only. It deliberately does
//! not perform bytecode type verification, name resolution, module lookup, or bootstrap-method
//! resolution. Every constant-pool index and module directive remains faithful input for later
//! verifier, linker, and runtime layers; none of the structures in this module has runtime meaning.

use core::fmt;

use crate::{ByteError, ByteReader, ByteWriter};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("attribute/basic.rs");
include!("attribute/annotations.rs");
include!("attribute/code.rs");
include!("attribute/class.rs");
