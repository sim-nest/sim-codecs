//! Bounded structural classfile shells and their separate typed validation projection.

use core::fmt;

use sim_kernel::{CodecId, Origin, SourceId, Span};

use crate::{
    ByteError, ByteReader, ByteWriter, CodeAttribute, Constant, ConstantPool, ConstantPoolError,
};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("shell/model.rs");
include!("shell/codec.rs");
