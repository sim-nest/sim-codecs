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
