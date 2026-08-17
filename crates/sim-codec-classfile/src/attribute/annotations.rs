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
