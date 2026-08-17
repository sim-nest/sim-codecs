//! Frozen scope contract for the bounded, lossless JVM classfile codec.
//!
//! Parsing is intentionally introduced only after the retained corpus and its
//! independently authored expectations have been fixed.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod attribute;
mod bytes;
mod constant;
mod encode;
mod instruction;
mod modified_utf8;
mod opcode_generated;
mod runtime;
mod shell;

pub use attribute::{
    Annotation, AnnotationDefaultAttribute, AnnotationElement, AnnotationsAttribute,
    AttributeError, AttributeErrorKind, AttributeOrigin, BootstrapMethod,
    BootstrapMethodsAttribute, ByteAttribute, CodeAttribute, CodeException, ElementValue,
    EnclosingMethodAttribute, IndexAttribute, IndexListAttribute, InnerClass,
    InnerClassesAttribute, LineNumber, LineNumberTableAttribute, LocalVariable,
    LocalVariableTarget, LocalVariablesAttribute, MAX_ANNOTATION_NESTING, MarkerAttribute,
    MethodParameter, MethodParametersAttribute, ModuleAttribute, ModuleExport, ModuleProvide,
    ModuleRequire, NestedAttribute, NestedAttributeOwner, ParameterAnnotationsAttribute,
    RecordAttribute, RecordComponent, RecordComponentAttribute, StackMapFrame,
    StackMapTableAttribute, TypeAnnotation, TypeAnnotationTarget, TypeAnnotationsAttribute,
    TypePathEntry, VerificationType, standard_attribute_min_major,
};
pub use bytes::{ByteError, ByteErrorKind, ByteReader, ByteWriter};
pub use constant::{
    Constant, ConstantPool, ConstantPoolError, ConstantPoolErrorKind, ConstantSlot,
};
pub use encode::encode_instructions;
pub use instruction::{
    DecodedCode, ExceptionHandlerRange, Instruction, InstructionError, InstructionErrorKind,
    InstructionId, InstructionOperand, LocatedInstruction, decode_instructions,
    validate_exception_handlers,
};
pub use modified_utf8::{decode_modified_utf8, encode_modified_utf8};
pub use opcode_generated::{OPCODES, Opcode, OpcodeMetadata};
pub use runtime::{ClassfileCodec, ClassfileCodecLib, inspect_classfile};
pub use shell::{
    AttributeLocation, AttributeOwner, AttributeShell, ClassIndex, ClassShell, EditReport,
    FieldShell, LayoutInvalidation, MethodShell, ShellBudget, ShellError, ShellErrorKind,
    Utf8Index, ValidatedClassShell, ValidatedFieldShell, ValidatedMethodShell,
};

/// Machine-readable format bounds and reuse decisions.
pub const SCOPE: &str = include_str!("../scope.toml");

/// Independently authored expectations for every retained fixture.
pub const FIXTURE_EXPECTATIONS: &str = include_str!("../fixtures/expectations.toml");

/// Cookbook recipes embedded for runtime help and browse surfaces.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
