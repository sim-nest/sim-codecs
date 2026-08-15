//! Frozen scope contract for the bounded, lossless JVM classfile codec.
//!
//! Parsing is intentionally introduced only after the retained corpus and its
//! independently authored expectations have been fixed.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bytes;
mod constant;
mod modified_utf8;
mod opcode_generated;
mod shell;

pub use bytes::{ByteError, ByteErrorKind, ByteReader, ByteWriter};
pub use constant::{
    Constant, ConstantPool, ConstantPoolError, ConstantPoolErrorKind, ConstantSlot,
};
pub use modified_utf8::{decode_modified_utf8, encode_modified_utf8};
pub use opcode_generated::{OPCODES, Opcode, OpcodeMetadata};
pub use shell::{
    AttributeShell, ClassIndex, ClassShell, FieldShell, MethodShell, ShellBudget, ShellError,
    ShellErrorKind, Utf8Index, ValidatedClassShell, ValidatedFieldShell, ValidatedMethodShell,
};

/// Machine-readable format bounds and reuse decisions.
pub const SCOPE: &str = include_str!("../scope.toml");

/// Independently authored expectations for every retained fixture.
pub const FIXTURE_EXPECTATIONS: &str = include_str!("../fixtures/expectations.toml");

#[cfg(test)]
mod tests;
