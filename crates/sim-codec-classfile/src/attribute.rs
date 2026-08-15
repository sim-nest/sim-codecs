//! Structured JVM attributes whose encoded order and indices are semantically significant.
//!
//! This module validates binary format and local static constraints only. It deliberately does
//! not perform bytecode type verification, name resolution, module lookup, or bootstrap-method
//! resolution. Every constant-pool index and module directive remains faithful input for later
//! verifier, linker, and runtime layers; none of the structures in this module has runtime meaning.

use core::fmt;

use crate::{ByteError, ByteReader, ByteWriter};

/// A stable structured-attribute failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeErrorKind {
    /// The bounded byte lane rejected the input or output.
    Bytes,
    /// A reserved stack-map frame tag or verification-type tag was encountered.
    ReservedTag,
    /// An attribute body contained trailing bytes.
    TrailingBytes,
    /// A collection cannot be represented by its classfile count field.
    CountOverflow,
    /// A locally checkable attribute constraint is invalid.
    StaticConstraint,
    /// A nested annotation value exceeded the caller's structural budget.
    NestingBudgetExceeded,
}

/// A located structured-attribute format error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeError {
    /// Stable machine-matchable failure category.
    pub kind: AttributeErrorKind,
    /// Absolute byte offset at which the failure was detected.
    pub offset: usize,
    /// Human-readable context.
    pub message: String,
}

impl fmt::Display for AttributeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for AttributeError {}

impl From<ByteError> for AttributeError {
    fn from(value: ByteError) -> Self {
        Self {
            kind: AttributeErrorKind::Bytes,
            offset: value.offset,
            message: value.message,
        }
    }
}

fn error(kind: AttributeErrorKind, offset: usize, message: impl Into<String>) -> AttributeError {
    AttributeError {
        kind,
        offset,
        message: message.into(),
    }
}

fn finish(reader: &ByteReader<'_>) -> Result<(), AttributeError> {
    if reader.remaining() == 0 {
        Ok(())
    } else {
        Err(error(
            AttributeErrorKind::TrailingBytes,
            reader.offset(),
            format!("{} trailing attribute bytes", reader.remaining()),
        ))
    }
}

fn count(value: usize, what: &str) -> Result<u16, AttributeError> {
    u16::try_from(value).map_err(|_| {
        error(
            AttributeErrorKind::CountOverflow,
            0,
            format!("too many {what}"),
        )
    })
}

fn read_u2s(reader: &mut ByteReader<'_>, what: &str) -> Result<Vec<u16>, AttributeError> {
    let n = usize::from(reader.read_u2()?);
    reader.preflight_allocation(n)?;
    let mut values = Vec::with_capacity(n);
    for _ in 0..n {
        values.push(reader.read_u2()?);
    }
    finish(reader)?;
    let _ = what;
    Ok(values)
}

fn write_u2s(values: &[u16], budget: usize, what: &str) -> Result<Vec<u8>, AttributeError> {
    let mut out = ByteWriter::new(budget);
    out.write_u2(count(values.len(), what)?)?;
    for value in values {
        out.write_u2(*value)?;
    }
    Ok(out.into_bytes())
}

/// A standard attribute whose payload is exactly one unresolved constant-pool index.
///
/// This represents `ConstantValue`, `Signature`, `SourceFile`, `NestHost`, and
/// `ModuleMainClass` metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexAttribute {
    /// The unresolved constant-pool index.
    pub index: u16,
}

impl IndexAttribute {
    /// Decode the exact two-byte payload.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let index = reader.read_u2()?;
        finish(reader)?;
        Ok(Self { index })
    }

    /// Encode the index without inspecting its target.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(self.index)?;
        Ok(out.into_bytes())
    }
}

/// A marker attribute (`Synthetic` or `Deprecated`), whose payload must be empty.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MarkerAttribute;

impl MarkerAttribute {
    /// Accept only an empty bounded payload.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        finish(reader)?;
        Ok(Self)
    }

    /// Encode the empty payload.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        Ok(ByteWriter::new(budget).into_bytes())
    }
}

/// An opaque byte payload, used by `SourceDebugExtension`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteAttribute {
    /// Exact bytes, without text decoding or newline normalization.
    pub bytes: Vec<u8>,
}

impl ByteAttribute {
    /// Retain all remaining bytes.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let bytes = reader.take(reader.remaining())?.to_vec();
        Ok(Self { bytes })
    }

    /// Encode the retained bytes exactly.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_bytes(&self.bytes)?;
        Ok(out.into_bytes())
    }
}

/// An ordered list of unresolved indices (`Exceptions`, `NestMembers`,
/// `PermittedSubclasses`, or `ModulePackages`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexListAttribute {
    /// Indices in classfile order.
    pub indices: Vec<u16>,
}

impl IndexListAttribute {
    /// Decode an unsigned-short-counted index list.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        Ok(Self {
            indices: read_u2s(reader, "indices")?,
        })
    }

    /// Encode the list without sorting or deduplication.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        write_u2s(&self.indices, budget, "indices")
    }
}

/// One `InnerClasses` table row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InnerClass {
    /// Class index for the nested class.
    pub inner_class_index: u16,
    /// Enclosing class index, or zero.
    pub outer_class_index: u16,
    /// Simple-name index, or zero for anonymous classes.
    pub inner_name_index: u16,
    /// Raw inner-class access flags.
    pub access_flags: u16,
}

/// The ordered `InnerClasses` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InnerClassesAttribute {
    /// Rows in declaration order.
    pub classes: Vec<InnerClass>,
}

impl InnerClassesAttribute {
    /// Decode all rows without resolving their indices.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut classes = Vec::with_capacity(n);
        for _ in 0..n {
            classes.push(InnerClass {
                inner_class_index: reader.read_u2()?,
                outer_class_index: reader.read_u2()?,
                inner_name_index: reader.read_u2()?,
                access_flags: reader.read_u2()?,
            });
        }
        finish(reader)?;
        Ok(Self { classes })
    }
    /// Encode rows exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.classes.len(), "inner classes")?)?;
        for v in &self.classes {
            out.write_u2(v.inner_class_index)?;
            out.write_u2(v.outer_class_index)?;
            out.write_u2(v.inner_name_index)?;
            out.write_u2(v.access_flags)?;
        }
        Ok(out.into_bytes())
    }
}

/// The `EnclosingMethod` payload; a zero method index denotes no specific method.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnclosingMethodAttribute {
    /// Enclosing class index.
    pub class_index: u16,
    /// Name-and-type index, or zero.
    pub method_index: u16,
}

impl EnclosingMethodAttribute {
    /// Decode the two unresolved indices.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let class_index = reader.read_u2()?;
        let method_index = reader.read_u2()?;
        finish(reader)?;
        Ok(Self {
            class_index,
            method_index,
        })
    }
    /// Encode the two indices.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(self.class_index)?;
        out.write_u2(self.method_index)?;
        Ok(out.into_bytes())
    }
}

/// One source line mapping in a `LineNumberTable`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineNumber {
    /// Code-array start offset.
    pub start_pc: u16,
    /// Source line number.
    pub line_number: u16,
}

/// An ordered `LineNumberTable` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineNumberTableAttribute {
    /// Mappings in encoded order.
    pub lines: Vec<LineNumber>,
}

impl LineNumberTableAttribute {
    /// Decode mappings without validating instruction boundaries.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut lines = Vec::with_capacity(n);
        for _ in 0..n {
            lines.push(LineNumber {
                start_pc: reader.read_u2()?,
                line_number: reader.read_u2()?,
            });
        }
        finish(reader)?;
        Ok(Self { lines })
    }
    /// Encode mappings exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.lines.len(), "line numbers")?)?;
        for v in &self.lines {
            out.write_u2(v.start_pc)?;
            out.write_u2(v.line_number)?;
        }
        Ok(out.into_bytes())
    }
}

/// One local-variable range, shared by `LocalVariableTable` and `LocalVariableTypeTable`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalVariable {
    /// Code-array start offset.
    pub start_pc: u16,
    /// Range length.
    pub length: u16,
    /// Name index.
    pub name_index: u16,
    /// Descriptor or signature index.
    pub type_index: u16,
    /// Local-variable slot.
    pub slot: u16,
}

/// An ordered local-variable table payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalVariablesAttribute {
    /// Ranges in encoded order.
    pub variables: Vec<LocalVariable>,
}

impl LocalVariablesAttribute {
    /// Decode ranges without resolving names or checking code offsets.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut variables = Vec::with_capacity(n);
        for _ in 0..n {
            variables.push(LocalVariable {
                start_pc: reader.read_u2()?,
                length: reader.read_u2()?,
                name_index: reader.read_u2()?,
                type_index: reader.read_u2()?,
                slot: reader.read_u2()?,
            });
        }
        finish(reader)?;
        Ok(Self { variables })
    }
    /// Encode ranges exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.variables.len(), "local variables")?)?;
        for v in &self.variables {
            out.write_u2(v.start_pc)?;
            out.write_u2(v.length)?;
            out.write_u2(v.name_index)?;
            out.write_u2(v.type_index)?;
            out.write_u2(v.slot)?;
        }
        Ok(out.into_bytes())
    }
}

/// One `MethodParameters` row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MethodParameter {
    /// Name index, or zero.
    pub name_index: u16,
    /// Raw parameter access flags.
    pub access_flags: u16,
}

/// The ordered `MethodParameters` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodParametersAttribute {
    /// Parameters in descriptor order.
    pub parameters: Vec<MethodParameter>,
}

impl MethodParametersAttribute {
    /// Decode the u1-counted parameter table.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u1()?);
        reader.preflight_allocation(n)?;
        let mut parameters = Vec::with_capacity(n);
        for _ in 0..n {
            parameters.push(MethodParameter {
                name_index: reader.read_u2()?,
                access_flags: reader.read_u2()?,
            });
        }
        finish(reader)?;
        Ok(Self { parameters })
    }
    /// Encode the parameter table.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u1(u8::try_from(self.parameters.len()).map_err(|_| {
            error(
                AttributeErrorKind::CountOverflow,
                0,
                "too many method parameters",
            )
        })?)?;
        for v in &self.parameters {
            out.write_u2(v.name_index)?;
            out.write_u2(v.access_flags)?;
        }
        Ok(out.into_bytes())
    }
}

/// Maximum annotation nesting accepted even when a caller supplies a larger budget.
///
/// This implementation ceiling keeps recursive parsing below the host stack's danger zone.
pub const MAX_ANNOTATION_NESTING: usize = 256;

/// The absolute half-open byte range occupied by one annotation structure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttributeOrigin {
    /// Absolute offset of the structure's first byte.
    pub start: usize,
    /// Absolute offset immediately after the structure's last byte.
    pub end: usize,
}

fn annotation_origin(start: usize, reader: &ByteReader<'_>) -> AttributeOrigin {
    AttributeOrigin {
        start,
        end: reader.offset(),
    }
}

/// One annotation element-name/value pair, retained in classfile order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationElement {
    /// Constant-pool index of the element name. Duplicate indices remain distinct entries.
    pub name_index: u16,
    /// Encoded element value.
    pub value: ElementValue,
    /// Source range of the complete pair.
    pub origin: AttributeOrigin,
}

/// One annotation, retaining its unresolved type index, element order, and source range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Annotation {
    /// Constant-pool index of the annotation interface descriptor.
    pub type_index: u16,
    /// Element pairs in encoded order, including duplicate names.
    pub elements: Vec<AnnotationElement>,
    /// Source range of the complete annotation.
    pub origin: AttributeOrigin,
}

/// A JVM annotation `element_value`, with every constant-pool index left unresolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ElementValue {
    /// Primitive or string constant (`B`, `C`, `D`, `F`, `I`, `J`, `S`, `Z`, or `s`).
    Constant {
        /// Exact element-value tag.
        tag: u8,
        /// Constant-pool index of the value.
        constant_index: u16,
        /// Source range of the complete value.
        origin: AttributeOrigin,
    },
    /// Enum constant (`e`).
    Enum {
        /// Constant-pool index of the enum type name.
        type_name_index: u16,
        /// Constant-pool index of the enum constant name.
        constant_name_index: u16,
        /// Source range of the complete value.
        origin: AttributeOrigin,
    },
    /// Class literal (`c`).
    Class {
        /// Constant-pool index of the return descriptor.
        class_info_index: u16,
        /// Source range of the complete value.
        origin: AttributeOrigin,
    },
    /// Nested annotation (`@`).
    Annotation {
        /// Nested annotation value.
        annotation: Box<Annotation>,
        /// Source range including the tag.
        origin: AttributeOrigin,
    },
    /// Ordered annotation array (`[`).
    Array {
        /// Values in encoded order.
        values: Vec<ElementValue>,
        /// Source range of the complete array.
        origin: AttributeOrigin,
    },
}

impl ElementValue {
    /// Source range occupied by this value.
    pub fn origin(&self) -> AttributeOrigin {
        match self {
            Self::Constant { origin, .. }
            | Self::Enum { origin, .. }
            | Self::Class { origin, .. }
            | Self::Annotation { origin, .. }
            | Self::Array { origin, .. } => *origin,
        }
    }
}

/// An ordered `RuntimeVisibleAnnotations` or `RuntimeInvisibleAnnotations` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationsAttribute {
    /// Annotations in declaration order.
    pub annotations: Vec<Annotation>,
}

impl AnnotationsAttribute {
    /// Decode a declaration-annotation payload under the supplied nesting budget.
    pub fn decode(
        reader: &mut ByteReader<'_>,
        nesting_budget: usize,
    ) -> Result<Self, AttributeError> {
        let annotations = decode_annotations(reader, nesting_budget)?;
        finish(reader)?;
        Ok(Self { annotations })
    }

    /// Encode without resolving or normalizing annotation indices.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        encode_annotations(&self.annotations, &mut out)?;
        Ok(out.into_bytes())
    }
}

/// An ordered `RuntimeVisibleParameterAnnotations` or
/// `RuntimeInvisibleParameterAnnotations` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterAnnotationsAttribute {
    /// Per-parameter annotation lists in descriptor parameter order.
    pub parameters: Vec<Vec<Annotation>>,
}

impl ParameterAnnotationsAttribute {
    /// Decode a parameter-annotation payload under the supplied nesting budget.
    pub fn decode(
        reader: &mut ByteReader<'_>,
        nesting_budget: usize,
    ) -> Result<Self, AttributeError> {
        let count = usize::from(reader.read_u1()?);
        reader.preflight_allocation(count)?;
        let mut parameters = Vec::with_capacity(count);
        for _ in 0..count {
            parameters.push(decode_annotations(reader, nesting_budget)?);
        }
        finish(reader)?;
        Ok(Self { parameters })
    }

    /// Encode parameter and annotation order exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u1(u8::try_from(self.parameters.len()).map_err(|_| {
            error(
                AttributeErrorKind::CountOverflow,
                0,
                "too many annotated parameters",
            )
        })?)?;
        for annotations in &self.parameters {
            encode_annotations(annotations, &mut out)?;
        }
        Ok(out.into_bytes())
    }
}

/// One step in a type annotation's path from the annotated type to its target component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypePathEntry {
    /// Path kind (array, nested, wildcard bound, or type argument).
    pub kind: u8,
    /// Type-argument index; zero for kinds other than type argument.
    pub argument_index: u8,
    /// Source range of this path entry.
    pub origin: AttributeOrigin,
}

/// The complete target-specific union from JVMS `target_info`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeAnnotationTarget {
    /// Class or method type parameter.
    TypeParameter {
        /// Type parameter index.
        index: u8,
    },
    /// Supertype, including `65535` for the superclass.
    Supertype {
        /// Supertype-table index.
        index: u16,
    },
    /// Bound of a class or method type parameter.
    TypeParameterBound {
        /// Type parameter index.
        parameter_index: u8,
        /// Bound index within the parameter.
        bound_index: u8,
    },
    /// Field, return type, or receiver target, whose union member is empty.
    Empty,
    /// Formal method parameter.
    FormalParameter {
        /// Formal parameter index.
        index: u8,
    },
    /// `throws` clause entry.
    Throws {
        /// Throws-table index.
        index: u16,
    },
    /// Local-variable table target, preserving table order and overlaps.
    LocalVariable {
        /// Ordered local-variable target table.
        table: Vec<LocalVariableTarget>,
    },
    /// Exception-table entry.
    Catch {
        /// Exception-table index.
        exception_table_index: u16,
    },
    /// `instanceof`, `new`, method reference, or constructor reference instruction.
    Offset {
        /// Instruction offset in the code array.
        offset: u16,
    },
    /// Cast/invocation/reference type argument.
    TypeArgument {
        /// Instruction offset in the code array.
        offset: u16,
        /// Type-argument index at the instruction.
        argument_index: u8,
    },
}

/// One local-variable target range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalVariableTarget {
    /// Inclusive code-array start offset.
    pub start_pc: u16,
    /// Range length in the code array.
    pub length: u16,
    /// Local-variable slot index.
    pub index: u16,
    /// Source range of this table row.
    pub origin: AttributeOrigin,
}

/// One type annotation, including its target union and component path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAnnotation {
    /// Exact target-type discriminator.
    pub target_type: u8,
    /// Target data selected by `target_type`.
    pub target: TypeAnnotationTarget,
    /// Component path in encoded order.
    pub path: Vec<TypePathEntry>,
    /// Annotation type index.
    pub type_index: u16,
    /// Ordered annotation element pairs.
    pub elements: Vec<AnnotationElement>,
    /// Source range of the complete type annotation.
    pub origin: AttributeOrigin,
}

/// An ordered runtime-visible or runtime-invisible type-annotation payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAnnotationsAttribute {
    /// Type annotations in encoded order.
    pub annotations: Vec<TypeAnnotation>,
}

impl TypeAnnotationsAttribute {
    /// Decode all target unions and paths under the supplied nesting budget.
    pub fn decode(
        reader: &mut ByteReader<'_>,
        nesting_budget: usize,
    ) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut annotations = Vec::with_capacity(n);
        for _ in 0..n {
            annotations.push(decode_type_annotation(reader, nesting_budget)?);
        }
        finish(reader)?;
        Ok(Self { annotations })
    }

    /// Encode target unions, paths, and elements exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.annotations.len(), "type annotations")?)?;
        for annotation in &self.annotations {
            encode_type_annotation(annotation, &mut out)?;
        }
        Ok(out.into_bytes())
    }
}

/// An `AnnotationDefault` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationDefaultAttribute {
    /// The method's default element value.
    pub value: ElementValue,
}

impl AnnotationDefaultAttribute {
    /// Decode the default value under the supplied nesting budget.
    pub fn decode(
        reader: &mut ByteReader<'_>,
        nesting_budget: usize,
    ) -> Result<Self, AttributeError> {
        let mut budget = NestingBudget::new(nesting_budget);
        let value = decode_element_value(reader, &mut budget)?;
        finish(reader)?;
        Ok(Self { value })
    }

    /// Encode the default value exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        encode_element_value(&self.value, &mut out)?;
        Ok(out.into_bytes())
    }
}

struct NestingBudget {
    remaining: usize,
}

impl NestingBudget {
    fn new(requested: usize) -> Self {
        Self {
            remaining: requested.min(MAX_ANNOTATION_NESTING),
        }
    }

    fn enter(&mut self, offset: usize) -> Result<(), AttributeError> {
        self.remaining = self.remaining.checked_sub(1).ok_or_else(|| {
            error(
                AttributeErrorKind::NestingBudgetExceeded,
                offset,
                "annotation nesting budget exceeded",
            )
        })?;
        Ok(())
    }

    fn leave(&mut self) {
        self.remaining += 1;
    }
}

fn decode_annotations(
    reader: &mut ByteReader<'_>,
    nesting_budget: usize,
) -> Result<Vec<Annotation>, AttributeError> {
    let n = usize::from(reader.read_u2()?);
    reader.preflight_allocation(n)?;
    let mut budget = NestingBudget::new(nesting_budget);
    let mut annotations = Vec::with_capacity(n);
    for _ in 0..n {
        annotations.push(decode_annotation(reader, &mut budget)?);
    }
    Ok(annotations)
}

fn decode_annotation(
    reader: &mut ByteReader<'_>,
    budget: &mut NestingBudget,
) -> Result<Annotation, AttributeError> {
    let start = reader.offset();
    let type_index = reader.read_u2()?;
    let n = usize::from(reader.read_u2()?);
    reader.preflight_allocation(n)?;
    let mut elements = Vec::with_capacity(n);
    for _ in 0..n {
        let pair_start = reader.offset();
        let name_index = reader.read_u2()?;
        let value = decode_element_value(reader, budget)?;
        elements.push(AnnotationElement {
            name_index,
            value,
            origin: annotation_origin(pair_start, reader),
        });
    }
    Ok(Annotation {
        type_index,
        elements,
        origin: annotation_origin(start, reader),
    })
}

fn decode_element_value(
    reader: &mut ByteReader<'_>,
    budget: &mut NestingBudget,
) -> Result<ElementValue, AttributeError> {
    let start = reader.offset();
    let tag = reader.read_u1()?;
    let value = match tag {
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' | b's' => ElementValue::Constant {
            tag,
            constant_index: reader.read_u2()?,
            origin: annotation_origin(start, reader),
        },
        b'e' => ElementValue::Enum {
            type_name_index: reader.read_u2()?,
            constant_name_index: reader.read_u2()?,
            origin: annotation_origin(start, reader),
        },
        b'c' => ElementValue::Class {
            class_info_index: reader.read_u2()?,
            origin: annotation_origin(start, reader),
        },
        b'@' => {
            budget.enter(start)?;
            let annotation = decode_annotation(reader, budget);
            budget.leave();
            ElementValue::Annotation {
                annotation: Box::new(annotation?),
                origin: annotation_origin(start, reader),
            }
        }
        b'[' => {
            budget.enter(start)?;
            let n = usize::from(reader.read_u2()?);
            // Depth is debited before this reservation, so rejected nested arrays never allocate.
            reader.preflight_allocation(n)?;
            let mut values = Vec::with_capacity(n);
            let result: Result<Vec<ElementValue>, AttributeError> = (|| {
                for _ in 0..n {
                    values.push(decode_element_value(reader, budget)?);
                }
                Ok(values)
            })();
            budget.leave();
            ElementValue::Array {
                values: result?,
                origin: annotation_origin(start, reader),
            }
        }
        _ => {
            return Err(error(
                AttributeErrorKind::ReservedTag,
                start,
                format!("reserved annotation element tag {tag}"),
            ));
        }
    };
    Ok(value)
}

fn encode_annotations(values: &[Annotation], out: &mut ByteWriter) -> Result<(), AttributeError> {
    out.write_u2(count(values.len(), "annotations")?)?;
    for value in values {
        encode_annotation(value, out)?;
    }
    Ok(())
}

fn encode_annotation(value: &Annotation, out: &mut ByteWriter) -> Result<(), AttributeError> {
    out.write_u2(value.type_index)?;
    out.write_u2(count(value.elements.len(), "annotation elements")?)?;
    for element in &value.elements {
        out.write_u2(element.name_index)?;
        encode_element_value(&element.value, out)?;
    }
    Ok(())
}

fn encode_element_value(value: &ElementValue, out: &mut ByteWriter) -> Result<(), AttributeError> {
    match value {
        ElementValue::Constant {
            tag,
            constant_index,
            ..
        } => {
            out.write_u1(*tag)?;
            out.write_u2(*constant_index)?;
        }
        ElementValue::Enum {
            type_name_index,
            constant_name_index,
            ..
        } => {
            out.write_u1(b'e')?;
            out.write_u2(*type_name_index)?;
            out.write_u2(*constant_name_index)?;
        }
        ElementValue::Class {
            class_info_index, ..
        } => {
            out.write_u1(b'c')?;
            out.write_u2(*class_info_index)?;
        }
        ElementValue::Annotation { annotation, .. } => {
            out.write_u1(b'@')?;
            encode_annotation(annotation, out)?;
        }
        ElementValue::Array { values, .. } => {
            out.write_u1(b'[')?;
            out.write_u2(count(values.len(), "annotation array values")?)?;
            for item in values {
                encode_element_value(item, out)?;
            }
        }
    }
    Ok(())
}

fn decode_type_annotation(
    reader: &mut ByteReader<'_>,
    nesting_budget: usize,
) -> Result<TypeAnnotation, AttributeError> {
    let start = reader.offset();
    let target_type = reader.read_u1()?;
    let target = match target_type {
        0x00 | 0x01 => TypeAnnotationTarget::TypeParameter {
            index: reader.read_u1()?,
        },
        0x10 => TypeAnnotationTarget::Supertype {
            index: reader.read_u2()?,
        },
        0x11 | 0x12 => TypeAnnotationTarget::TypeParameterBound {
            parameter_index: reader.read_u1()?,
            bound_index: reader.read_u1()?,
        },
        0x13..=0x15 => TypeAnnotationTarget::Empty,
        0x16 => TypeAnnotationTarget::FormalParameter {
            index: reader.read_u1()?,
        },
        0x17 => TypeAnnotationTarget::Throws {
            index: reader.read_u2()?,
        },
        0x40 | 0x41 => {
            let n = usize::from(reader.read_u2()?);
            reader.preflight_allocation(n)?;
            let mut table = Vec::with_capacity(n);
            for _ in 0..n {
                let row_start = reader.offset();
                table.push(LocalVariableTarget {
                    start_pc: reader.read_u2()?,
                    length: reader.read_u2()?,
                    index: reader.read_u2()?,
                    origin: annotation_origin(row_start, reader),
                });
            }
            TypeAnnotationTarget::LocalVariable { table }
        }
        0x42 => TypeAnnotationTarget::Catch {
            exception_table_index: reader.read_u2()?,
        },
        0x43..=0x46 => TypeAnnotationTarget::Offset {
            offset: reader.read_u2()?,
        },
        0x47..=0x4b => TypeAnnotationTarget::TypeArgument {
            offset: reader.read_u2()?,
            argument_index: reader.read_u1()?,
        },
        _ => {
            return Err(error(
                AttributeErrorKind::ReservedTag,
                start,
                format!("reserved type annotation target {target_type:#04x}"),
            ));
        }
    };
    let path_len = usize::from(reader.read_u1()?);
    reader.preflight_allocation(path_len)?;
    let mut path = Vec::with_capacity(path_len);
    for _ in 0..path_len {
        let path_start = reader.offset();
        let kind = reader.read_u1()?;
        let argument_index = reader.read_u1()?;
        if kind > 3 || (kind != 3 && argument_index != 0) {
            return Err(error(
                AttributeErrorKind::StaticConstraint,
                path_start,
                "invalid type annotation path entry",
            ));
        }
        path.push(TypePathEntry {
            kind,
            argument_index,
            origin: annotation_origin(path_start, reader),
        });
    }
    let mut budget = NestingBudget::new(nesting_budget);
    let annotation = decode_annotation(reader, &mut budget)?;
    Ok(TypeAnnotation {
        target_type,
        target,
        path,
        type_index: annotation.type_index,
        elements: annotation.elements,
        origin: annotation_origin(start, reader),
    })
}

fn encode_type_annotation(
    value: &TypeAnnotation,
    out: &mut ByteWriter,
) -> Result<(), AttributeError> {
    out.write_u1(value.target_type)?;
    match (&value.target, value.target_type) {
        (TypeAnnotationTarget::TypeParameter { index }, 0x00 | 0x01) => out.write_u1(*index)?,
        (TypeAnnotationTarget::Supertype { index }, 0x10) => out.write_u2(*index)?,
        (
            TypeAnnotationTarget::TypeParameterBound {
                parameter_index,
                bound_index,
            },
            0x11 | 0x12,
        ) => {
            out.write_u1(*parameter_index)?;
            out.write_u1(*bound_index)?;
        }
        (TypeAnnotationTarget::Empty, 0x13..=0x15) => {}
        (TypeAnnotationTarget::FormalParameter { index }, 0x16) => out.write_u1(*index)?,
        (TypeAnnotationTarget::Throws { index }, 0x17) => out.write_u2(*index)?,
        (TypeAnnotationTarget::LocalVariable { table }, 0x40 | 0x41) => {
            out.write_u2(count(table.len(), "local-variable targets")?)?;
            for row in table {
                out.write_u2(row.start_pc)?;
                out.write_u2(row.length)?;
                out.write_u2(row.index)?;
            }
        }
        (
            TypeAnnotationTarget::Catch {
                exception_table_index,
            },
            0x42,
        ) => out.write_u2(*exception_table_index)?,
        (TypeAnnotationTarget::Offset { offset }, 0x43..=0x46) => out.write_u2(*offset)?,
        (
            TypeAnnotationTarget::TypeArgument {
                offset,
                argument_index,
            },
            0x47..=0x4b,
        ) => {
            out.write_u2(*offset)?;
            out.write_u1(*argument_index)?;
        }
        _ => {
            return Err(error(
                AttributeErrorKind::StaticConstraint,
                0,
                "type annotation target does not match target_type",
            ));
        }
    }
    out.write_u1(u8::try_from(value.path.len()).map_err(|_| {
        error(
            AttributeErrorKind::CountOverflow,
            0,
            "type path is too long",
        )
    })?)?;
    for entry in &value.path {
        if entry.kind > 3 || (entry.kind != 3 && entry.argument_index != 0) {
            return Err(error(
                AttributeErrorKind::StaticConstraint,
                0,
                "invalid type annotation path entry",
            ));
        }
        out.write_u1(entry.kind)?;
        out.write_u1(entry.argument_index)?;
    }
    encode_annotation(
        &Annotation {
            type_index: value.type_index,
            elements: value.elements.clone(),
            origin: value.origin,
        },
        out,
    )
}

/// A nested attribute retained in declaration order with an unresolved name index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NestedAttribute {
    /// Constant-pool index naming the attribute.
    pub name_index: u16,
    /// Owner category in which this attribute was decoded.
    pub owner: NestedAttributeOwner,
    /// Zero-based order within its owner's table.
    pub order: usize,
    /// Body length declared by the attribute header.
    pub declared_length: u32,
    /// Exact attribute payload.
    pub bytes: Vec<u8>,
    /// Source range covering the complete header and body.
    pub origin: AttributeOrigin,
}

/// Legal owners for attributes nested inside structured attribute bodies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NestedAttributeOwner {
    /// A `Code` attribute.
    Code,
    /// A record component.
    RecordComponent,
}

/// One `Code` exception-table row, with all offsets and the catch index retained verbatim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeException {
    /// Inclusive bytecode start offset.
    pub start_pc: u16,
    /// Exclusive bytecode end offset.
    pub end_pc: u16,
    /// Handler bytecode offset.
    pub handler_pc: u16,
    /// Constant-pool class index, or zero for a catch-all handler.
    pub catch_type: u16,
}

/// The ordered, index-preserving body of a JVM `Code` attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeAttribute {
    /// Declared operand-stack bound.
    pub max_stack: u16,
    /// Declared local-variable bound.
    pub max_locals: u16,
    /// Exact bytecode array; instruction decoding remains a separate operation.
    pub code: Vec<u8>,
    /// Exception handlers in classfile order.
    pub exception_table: Vec<CodeException>,
    /// Nested attributes in classfile order.
    pub attributes: Vec<NestedAttribute>,
}

impl CodeAttribute {
    /// Decode a complete `Code` payload, checking only its binary shape and allocation budget.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let max_stack = reader.read_u2()?;
        let max_locals = reader.read_u2()?;
        let code_len = usize::try_from(reader.read_u4()?).map_err(|_| {
            error(
                AttributeErrorKind::CountOverflow,
                reader.offset(),
                "code length is not addressable",
            )
        })?;
        reader.preflight_allocation(code_len)?;
        let code = reader.take(code_len)?.to_vec();
        let exception_count = usize::from(reader.read_u2()?);
        reader.preflight_allocation(exception_count)?;
        let mut exception_table = Vec::with_capacity(exception_count);
        for _ in 0..exception_count {
            exception_table.push(CodeException {
                start_pc: reader.read_u2()?,
                end_pc: reader.read_u2()?,
                handler_pc: reader.read_u2()?,
                catch_type: reader.read_u2()?,
            });
        }
        validate_code_shape(code.len(), &exception_table)?;
        let attribute_count = usize::from(reader.read_u2()?);
        reader.preflight_allocation(attribute_count)?;
        let mut attributes = Vec::with_capacity(attribute_count);
        for order in 0..attribute_count {
            let start = reader.offset();
            let name_index = reader.read_u2()?;
            let declared_length = reader.read_u4()?;
            let length = usize::try_from(declared_length).map_err(|_| {
                error(
                    AttributeErrorKind::CountOverflow,
                    reader.offset(),
                    "nested attribute length is not addressable",
                )
            })?;
            reader.preflight_allocation(length)?;
            attributes.push(NestedAttribute {
                name_index,
                owner: NestedAttributeOwner::Code,
                order,
                declared_length,
                bytes: reader.take(length)?.to_vec(),
                origin: annotation_origin(start, reader),
            });
        }
        finish(reader)?;
        Ok(Self {
            max_stack,
            max_locals,
            code,
            exception_table,
            attributes,
        })
    }

    /// Encode the payload without resolving indices or performing bytecode verification.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        validate_code_shape(self.code.len(), &self.exception_table)?;
        let mut out = ByteWriter::new(budget);
        out.write_u2(self.max_stack)?;
        out.write_u2(self.max_locals)?;
        out.write_u4(
            u32::try_from(self.code.len())
                .map_err(|_| error(AttributeErrorKind::CountOverflow, 0, "code is too long"))?,
        )?;
        out.write_bytes(&self.code)?;
        out.write_u2(count(self.exception_table.len(), "exception handlers")?)?;
        for row in &self.exception_table {
            out.write_u2(row.start_pc)?;
            out.write_u2(row.end_pc)?;
            out.write_u2(row.handler_pc)?;
            out.write_u2(row.catch_type)?;
        }
        out.write_u2(count(self.attributes.len(), "nested attributes")?)?;
        for attribute in &self.attributes {
            if usize::try_from(attribute.declared_length).ok() != Some(attribute.bytes.len()) {
                return Err(error(
                    AttributeErrorKind::StaticConstraint,
                    attribute.origin.start,
                    "nested attribute declared length differs from retained bytes",
                ));
            }
            out.write_u2(attribute.name_index)?;
            out.write_u4(attribute.declared_length)?;
            out.write_bytes(&attribute.bytes)?;
        }
        Ok(out.into_bytes())
    }
}

fn validate_code_shape(
    code_length: usize,
    exceptions: &[CodeException],
) -> Result<(), AttributeError> {
    if !(1..=u16::MAX as usize).contains(&code_length) {
        return Err(error(
            AttributeErrorKind::StaticConstraint,
            0,
            format!("Code array length {code_length} is outside 1..=65535"),
        ));
    }
    for exception in exceptions {
        let start = usize::from(exception.start_pc);
        let end = usize::from(exception.end_pc);
        let handler = usize::from(exception.handler_pc);
        if start >= end || end > code_length || handler >= code_length {
            return Err(error(
                AttributeErrorKind::StaticConstraint,
                start,
                format!(
                    "exception range {start}..{end} with handler {handler} is outside Code length {code_length}"
                ),
            ));
        }
    }
    Ok(())
}

/// A verifier type exactly as represented by `verification_type_info`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationType {
    /// `Top_variable_info`.
    Top,
    /// `Integer_variable_info`.
    Integer,
    /// `Float_variable_info`.
    Float,
    /// `Double_variable_info`.
    Double,
    /// `Long_variable_info`.
    Long,
    /// `Null_variable_info`.
    Null,
    /// `UninitializedThis_variable_info`.
    UninitializedThis,
    /// `Object_variable_info`, retaining its constant-pool index.
    Object(u16),
    /// `Uninitialized_variable_info`, retaining its `new` instruction offset verbatim.
    Uninitialized(u16),
}

/// One compressed stack-map frame, retained without expansion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StackMapFrame {
    /// Tags 0 through 63.
    Same {
        /// Encoded frame tag, which is also the offset delta.
        frame_type: u8,
    },
    /// Tags 64 through 127.
    SameLocalsOneStack {
        /// Encoded frame tag.
        frame_type: u8,
        /// Sole stack entry.
        stack: VerificationType,
    },
    /// Tag 247.
    SameLocalsOneStackExtended {
        /// Explicit offset delta.
        offset_delta: u16,
        /// Sole stack entry.
        stack: VerificationType,
    },
    /// Tags 248 through 250.
    Chop {
        /// Encoded frame tag, retaining the exact number of omitted locals.
        frame_type: u8,
        /// Explicit offset delta.
        offset_delta: u16,
    },
    /// Tag 251.
    SameExtended {
        /// Explicit offset delta.
        offset_delta: u16,
    },
    /// Tags 252 through 254.
    Append {
        /// Encoded frame tag, retaining the exact number of appended locals.
        frame_type: u8,
        /// Explicit offset delta.
        offset_delta: u16,
        /// Appended locals in encoded order.
        locals: Vec<VerificationType>,
    },
    /// Tag 255.
    Full {
        /// Explicit offset delta.
        offset_delta: u16,
        /// Complete locals in encoded order.
        locals: Vec<VerificationType>,
        /// Complete stack in encoded order.
        stack: Vec<VerificationType>,
    },
}

/// An ordered `StackMapTable` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackMapTableAttribute {
    /// Frames in their original compressed representation.
    pub frames: Vec<StackMapFrame>,
}

impl StackMapTableAttribute {
    /// Decode a complete stack-map payload without performing type-state verification.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut frames = Vec::with_capacity(n);
        for _ in 0..n {
            frames.push(decode_frame(reader)?);
        }
        finish(reader)?;
        Ok(Self { frames })
    }
    /// Encode frames in their retained compressed forms.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.frames.len(), "stack-map frames")?)?;
        for frame in &self.frames {
            encode_frame(frame, &mut out)?;
        }
        Ok(out.into_bytes())
    }
}

fn decode_type(reader: &mut ByteReader<'_>) -> Result<VerificationType, AttributeError> {
    let at = reader.offset();
    Ok(match reader.read_u1()? {
        0 => VerificationType::Top,
        1 => VerificationType::Integer,
        2 => VerificationType::Float,
        3 => VerificationType::Double,
        4 => VerificationType::Long,
        5 => VerificationType::Null,
        6 => VerificationType::UninitializedThis,
        7 => VerificationType::Object(reader.read_u2()?),
        8 => VerificationType::Uninitialized(reader.read_u2()?),
        tag => {
            return Err(error(
                AttributeErrorKind::ReservedTag,
                at,
                format!("reserved verification type tag {tag}"),
            ));
        }
    })
}

fn encode_type(value: VerificationType, out: &mut ByteWriter) -> Result<(), AttributeError> {
    let (tag, extra) = match value {
        VerificationType::Top => (0, None),
        VerificationType::Integer => (1, None),
        VerificationType::Float => (2, None),
        VerificationType::Double => (3, None),
        VerificationType::Long => (4, None),
        VerificationType::Null => (5, None),
        VerificationType::UninitializedThis => (6, None),
        VerificationType::Object(v) => (7, Some(v)),
        VerificationType::Uninitialized(v) => (8, Some(v)),
    };
    out.write_u1(tag)?;
    if let Some(v) = extra {
        out.write_u2(v)?;
    }
    Ok(())
}

fn decode_frame(r: &mut ByteReader<'_>) -> Result<StackMapFrame, AttributeError> {
    let at = r.offset();
    let tag = r.read_u1()?;
    Ok(match tag {
        0..=63 => StackMapFrame::Same { frame_type: tag },
        64..=127 => StackMapFrame::SameLocalsOneStack {
            frame_type: tag,
            stack: decode_type(r)?,
        },
        128..=246 => {
            return Err(error(
                AttributeErrorKind::ReservedTag,
                at,
                format!("reserved stack-map frame tag {tag}"),
            ));
        }
        247 => StackMapFrame::SameLocalsOneStackExtended {
            offset_delta: r.read_u2()?,
            stack: decode_type(r)?,
        },
        248..=250 => StackMapFrame::Chop {
            frame_type: tag,
            offset_delta: r.read_u2()?,
        },
        251 => StackMapFrame::SameExtended {
            offset_delta: r.read_u2()?,
        },
        252..=254 => {
            let offset_delta = r.read_u2()?;
            let mut locals = Vec::with_capacity(usize::from(tag - 251));
            for _ in 0..tag - 251 {
                locals.push(decode_type(r)?);
            }
            StackMapFrame::Append {
                frame_type: tag,
                offset_delta,
                locals,
            }
        }
        255 => {
            let offset_delta = r.read_u2()?;
            let nl = usize::from(r.read_u2()?);
            r.preflight_allocation(nl)?;
            let mut locals = Vec::with_capacity(nl);
            for _ in 0..nl {
                locals.push(decode_type(r)?);
            }
            let ns = usize::from(r.read_u2()?);
            r.preflight_allocation(ns)?;
            let mut stack = Vec::with_capacity(ns);
            for _ in 0..ns {
                stack.push(decode_type(r)?);
            }
            StackMapFrame::Full {
                offset_delta,
                locals,
                stack,
            }
        }
    })
}

fn encode_frame(f: &StackMapFrame, out: &mut ByteWriter) -> Result<(), AttributeError> {
    match f {
        StackMapFrame::Same { frame_type: t } if *t <= 63 => out.write_u1(*t)?,
        StackMapFrame::SameLocalsOneStack {
            frame_type: t,
            stack,
        } if (64..=127).contains(t) => {
            out.write_u1(*t)?;
            encode_type(*stack, out)?
        }
        StackMapFrame::SameLocalsOneStackExtended {
            offset_delta,
            stack,
        } => {
            out.write_u1(247)?;
            out.write_u2(*offset_delta)?;
            encode_type(*stack, out)?
        }
        StackMapFrame::Chop {
            frame_type: t,
            offset_delta,
        } if (248..=250).contains(t) => {
            out.write_u1(*t)?;
            out.write_u2(*offset_delta)?
        }
        StackMapFrame::SameExtended { offset_delta } => {
            out.write_u1(251)?;
            out.write_u2(*offset_delta)?
        }
        StackMapFrame::Append {
            frame_type: t,
            offset_delta,
            locals,
        } if (252..=254).contains(t) && locals.len() == usize::from(*t - 251) => {
            out.write_u1(*t)?;
            out.write_u2(*offset_delta)?;
            for v in locals {
                encode_type(*v, out)?
            }
        }
        StackMapFrame::Full {
            offset_delta,
            locals,
            stack,
        } => {
            out.write_u1(255)?;
            out.write_u2(*offset_delta)?;
            out.write_u2(count(locals.len(), "full-frame locals")?)?;
            for v in locals {
                encode_type(*v, out)?
            }
            out.write_u2(count(stack.len(), "full-frame stack entries")?)?;
            for v in stack {
                encode_type(*v, out)?
            }
        }
        _ => {
            return Err(error(
                AttributeErrorKind::ReservedTag,
                0,
                "frame variant contains a tag or arity outside its static format",
            ));
        }
    }
    Ok(())
}

/// One bootstrap method and its ordered, arity-preserving constant-pool arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapMethod {
    /// Constant-pool index of the method handle.
    pub method_ref: u16,
    /// Constant-pool argument indices in invocation order.
    pub arguments: Vec<u16>,
}

/// An ordered `BootstrapMethods` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapMethodsAttribute {
    /// Bootstrap methods in classfile index order.
    pub methods: Vec<BootstrapMethod>,
}

impl BootstrapMethodsAttribute {
    /// Decode without resolving method handles, arguments, or dynamic constants.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut methods = Vec::with_capacity(n);
        for _ in 0..n {
            let method_ref = reader.read_u2()?;
            let argc = usize::from(reader.read_u2()?);
            reader.preflight_allocation(argc)?;
            let mut arguments = Vec::with_capacity(argc);
            for _ in 0..argc {
                arguments.push(reader.read_u2()?)
            }
            methods.push(BootstrapMethod {
                method_ref,
                arguments,
            });
        }
        finish(reader)?;
        Ok(Self { methods })
    }
    /// Encode method and argument sequences exactly in stored order.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.methods.len(), "bootstrap methods")?)?;
        for method in &self.methods {
            out.write_u2(method.method_ref)?;
            out.write_u2(count(method.arguments.len(), "bootstrap arguments")?)?;
            for argument in &method.arguments {
                out.write_u2(*argument)?
            }
        }
        Ok(out.into_bytes())
    }
}

/// One nested attribute attached to a record component.
pub type RecordComponentAttribute = NestedAttribute;

/// One component in a `Record` attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordComponent {
    /// Component name index.
    pub name_index: u16,
    /// Component descriptor index.
    pub descriptor_index: u16,
    /// Component attributes in classfile order.
    pub attributes: Vec<RecordComponentAttribute>,
}

/// The ordered `Record` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordAttribute {
    /// Components in declaration order.
    pub components: Vec<RecordComponent>,
}

impl RecordAttribute {
    /// Decode components and retain every nested attribute as exact bytes.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut components = Vec::with_capacity(n);
        for _ in 0..n {
            let name_index = reader.read_u2()?;
            let descriptor_index = reader.read_u2()?;
            let attribute_count = usize::from(reader.read_u2()?);
            reader.preflight_allocation(attribute_count)?;
            let mut attributes = Vec::with_capacity(attribute_count);
            for order in 0..attribute_count {
                let start = reader.offset();
                let name_index = reader.read_u2()?;
                let declared_length = reader.read_u4()?;
                let length = usize::try_from(declared_length).map_err(|_| {
                    error(
                        AttributeErrorKind::StaticConstraint,
                        reader.offset(),
                        "record component attribute length is not addressable",
                    )
                })?;
                attributes.push(NestedAttribute {
                    name_index,
                    owner: NestedAttributeOwner::RecordComponent,
                    order,
                    declared_length,
                    bytes: reader.take(length)?.to_vec(),
                    origin: annotation_origin(start, reader),
                });
            }
            components.push(RecordComponent {
                name_index,
                descriptor_index,
                attributes,
            });
        }
        finish(reader)?;
        Ok(Self { components })
    }

    /// Encode component and nested-attribute order exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.components.len(), "record components")?)?;
        for component in &self.components {
            out.write_u2(component.name_index)?;
            out.write_u2(component.descriptor_index)?;
            out.write_u2(count(
                component.attributes.len(),
                "record component attributes",
            )?)?;
            for attribute in &component.attributes {
                if usize::try_from(attribute.declared_length).ok() != Some(attribute.bytes.len()) {
                    return Err(error(
                        AttributeErrorKind::StaticConstraint,
                        attribute.origin.start,
                        "record component attribute declared length differs from retained bytes",
                    ));
                }
                out.write_u2(attribute.name_index)?;
                out.write_u4(attribute.declared_length)?;
                out.write_bytes(&attribute.bytes)?;
            }
        }
        Ok(out.into_bytes())
    }
}

/// One `requires` directive in a `Module` attribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRequire {
    /// Module constant index.
    pub module_index: u16,
    /// Raw requires flags.
    pub flags: u16,
    /// Version string index, or zero.
    pub version_index: u16,
}
/// One `exports` or `opens` directive in a `Module` attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleExport {
    /// Package constant index.
    pub package_index: u16,
    /// Raw directive flags.
    pub flags: u16,
    /// Target module indices in encoded order.
    pub targets: Vec<u16>,
}
/// One `provides` directive in a `Module` attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleProvide {
    /// Service class index.
    pub service_index: u16,
    /// Provider class indices in encoded order.
    pub providers: Vec<u16>,
}
/// The complete structural `Module` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleAttribute {
    /// Module constant index.
    pub name_index: u16,
    /// Raw module flags.
    pub flags: u16,
    /// Module version string index, or zero.
    pub version_index: u16,
    /// Required modules in declaration order.
    pub requires: Vec<ModuleRequire>,
    /// Export directives in declaration order.
    pub exports: Vec<ModuleExport>,
    /// Open directives in declaration order.
    pub opens: Vec<ModuleExport>,
    /// Used service class indices in declaration order.
    pub uses: Vec<u16>,
    /// Service provider directives in declaration order.
    pub provides: Vec<ModuleProvide>,
}

fn decode_module_exports(reader: &mut ByteReader<'_>) -> Result<Vec<ModuleExport>, AttributeError> {
    let n = usize::from(reader.read_u2()?);
    reader.preflight_allocation(n)?;
    let mut values = Vec::with_capacity(n);
    for _ in 0..n {
        let package_index = reader.read_u2()?;
        let flags = reader.read_u2()?;
        let m = usize::from(reader.read_u2()?);
        reader.preflight_allocation(m)?;
        let mut targets = Vec::with_capacity(m);
        for _ in 0..m {
            targets.push(reader.read_u2()?);
        }
        values.push(ModuleExport {
            package_index,
            flags,
            targets,
        });
    }
    Ok(values)
}
fn encode_module_exports(
    values: &[ModuleExport],
    out: &mut ByteWriter,
    what: &str,
) -> Result<(), AttributeError> {
    out.write_u2(count(values.len(), what)?)?;
    for v in values {
        out.write_u2(v.package_index)?;
        out.write_u2(v.flags)?;
        out.write_u2(count(v.targets.len(), "module directive targets")?)?;
        for target in &v.targets {
            out.write_u2(*target)?;
        }
    }
    Ok(())
}

impl ModuleAttribute {
    /// Decode all module directives without resolving or interpreting them.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let name_index = reader.read_u2()?;
        let flags = reader.read_u2()?;
        let version_index = reader.read_u2()?;
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut requires = Vec::with_capacity(n);
        for _ in 0..n {
            requires.push(ModuleRequire {
                module_index: reader.read_u2()?,
                flags: reader.read_u2()?,
                version_index: reader.read_u2()?,
            });
        }
        let exports = decode_module_exports(reader)?;
        let opens = decode_module_exports(reader)?;
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut uses = Vec::with_capacity(n);
        for _ in 0..n {
            uses.push(reader.read_u2()?);
        }
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut provides = Vec::with_capacity(n);
        for _ in 0..n {
            let service_index = reader.read_u2()?;
            let m = usize::from(reader.read_u2()?);
            reader.preflight_allocation(m)?;
            let mut providers = Vec::with_capacity(m);
            for _ in 0..m {
                providers.push(reader.read_u2()?);
            }
            provides.push(ModuleProvide {
                service_index,
                providers,
            });
        }
        finish(reader)?;
        Ok(Self {
            name_index,
            flags,
            version_index,
            requires,
            exports,
            opens,
            uses,
            provides,
        })
    }
    /// Encode every directive exactly in stored order.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(self.name_index)?;
        out.write_u2(self.flags)?;
        out.write_u2(self.version_index)?;
        out.write_u2(count(self.requires.len(), "module requires")?)?;
        for v in &self.requires {
            out.write_u2(v.module_index)?;
            out.write_u2(v.flags)?;
            out.write_u2(v.version_index)?;
        }
        encode_module_exports(&self.exports, &mut out, "module exports")?;
        encode_module_exports(&self.opens, &mut out, "module opens")?;
        out.write_u2(count(self.uses.len(), "module uses")?)?;
        for v in &self.uses {
            out.write_u2(*v)?;
        }
        out.write_u2(count(self.provides.len(), "module provides")?)?;
        for v in &self.provides {
            out.write_u2(v.service_index)?;
            out.write_u2(count(v.providers.len(), "module providers")?)?;
            for provider in &v.providers {
                out.write_u2(*provider)?;
            }
        }
        Ok(out.into_bytes())
    }
}

/// Earliest classfile major version for a standard attribute.
pub fn standard_attribute_min_major(name: &str) -> Option<u16> {
    match name {
        "Signature"
        | "SourceDebugExtension"
        | "LocalVariableTypeTable"
        | "EnclosingMethod"
        | "RuntimeVisibleAnnotations"
        | "RuntimeInvisibleAnnotations"
        | "RuntimeVisibleParameterAnnotations"
        | "RuntimeInvisibleParameterAnnotations"
        | "AnnotationDefault" => Some(49),
        "StackMapTable" => Some(50),
        "BootstrapMethods" => Some(51),
        "MethodParameters"
        | "RuntimeVisibleTypeAnnotations"
        | "RuntimeInvisibleTypeAnnotations" => Some(52),
        "Module" | "ModulePackages" | "ModuleMainClass" => Some(53),
        "NestHost" | "NestMembers" => Some(55),
        "Record" => Some(60),
        "PermittedSubclasses" => Some(61),
        "ConstantValue" | "Code" | "Exceptions" | "InnerClasses" | "Synthetic" | "SourceFile"
        | "LineNumberTable" | "LocalVariableTable" | "Deprecated" => Some(45),
        _ => None,
    }
}
