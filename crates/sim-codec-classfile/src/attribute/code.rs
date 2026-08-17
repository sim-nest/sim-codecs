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
