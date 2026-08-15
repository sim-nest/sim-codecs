//! Manifest-driven decoding of JVM instruction streams.

use core::fmt;
use std::collections::BTreeMap;

use crate::{Constant, ConstantPool, Opcode};

/// Stable identity assigned in bytecode order within one decoded code array.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InstructionId(pub u32);

/// One decoded operand, retaining its semantic role from the opcode manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionOperand {
    /// A signed immediate value.
    Immediate(i32),
    /// A local-variable index.
    Local(u16),
    /// A constant-pool index.
    Constant(u16),
    /// A relative branch displacement.
    Branch(i32),
    /// The `invokeinterface` argument count.
    Count(u8),
    /// The `multianewarray` dimension count.
    Dimensions(u8),
    /// The primitive array type discriminator.
    ArrayType(u8),
}

/// A decoded instruction whose shape was selected by generated manifest metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Instruction {
    /// Effective opcode (the opcode following `wide` for a widened instruction).
    pub opcode: Opcode,
    /// Whether the instruction was encoded with the `wide` prefix.
    pub wide: bool,
    /// Decoded operands in manifest order.
    pub operands: Vec<InstructionOperand>,
}

/// An instruction paired with its stable identity and first byte offset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedInstruction {
    /// Stable stream-local identity.
    pub id: InstructionId,
    /// Offset of the opcode, or of the `wide` prefix when present.
    pub offset: u32,
    /// Decoded instruction.
    pub instruction: Instruction,
}

/// A decoded code array and its exact first-byte lookup map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedCode {
    /// Instructions in bytecode order.
    pub instructions: Vec<LocatedInstruction>,
    /// Map from every instruction's first byte offset to its stable identity.
    pub offsets: BTreeMap<u32, InstructionId>,
}

/// Stable instruction decoding failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionErrorKind {
    /// An operand extends beyond the code array.
    Truncated,
    /// The opcode is reserved or otherwise invalid in a classfile.
    InvalidOpcode,
    /// The opcode is unavailable in the requested classfile version.
    Version,
    /// A mandated reserved operand byte was nonzero.
    ReservedByte,
    /// The constant-pool index has the wrong category or is unusable.
    ConstantPool,
    /// A `wide` prefix modifies an opcode that cannot be widened.
    IllegalWide,
    /// Variable switch decoding belongs to the offset-sensitive decoder.
    VariableLayout,
    /// Generated manifest metadata is internally inconsistent.
    Manifest,
}

/// A byte-located instruction decoding error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionError {
    /// Stable failure category.
    pub kind: InstructionErrorKind,
    /// First byte of the failing instruction.
    pub offset: u32,
    /// Human-readable context.
    pub message: String,
}

impl fmt::Display for InstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at code offset {}", self.message, self.offset)
    }
}

impl std::error::Error for InstructionError {}

/// Decode the common JVM instruction layouts using only generated opcode metadata.
pub fn decode_instructions(
    code: &[u8],
    major_version: u16,
    pool: &ConstantPool,
) -> Result<DecodedCode, InstructionError> {
    let mut cursor = 0usize;
    let mut instructions = Vec::new();
    let mut offsets = BTreeMap::new();
    while cursor < code.len() {
        let start = cursor;
        let first = read_u1(code, &mut cursor, start)?;
        let mut opcode = Opcode::from_byte(first);
        let mut metadata = opcode.metadata();
        check_metadata(metadata, major_version, start)?;
        let wide = metadata.operands.starts_with("modified_opcode:");
        if wide {
            opcode = Opcode::from_byte(read_u1(code, &mut cursor, start)?);
            metadata = opcode.metadata();
            check_metadata(metadata, major_version, start)?;
            if metadata.operands != "local:u1" && metadata.operands != "local:u1,increment:s1" {
                return Err(error(
                    InstructionErrorKind::IllegalWide,
                    start,
                    format!("wide cannot modify {}", metadata.mnemonic),
                ));
            }
        }
        if metadata.width == "variable" {
            return Err(error(
                InstructionErrorKind::VariableLayout,
                start,
                format!(
                    "offset-sensitive {} layout is not a common fixed layout",
                    metadata.mnemonic
                ),
            ));
        }
        let mut operands = Vec::new();
        for field in metadata.operands.split(',') {
            match field {
                "none" => {}
                "value:s1" | "increment:s1" if !wide => operands.push(
                    InstructionOperand::Immediate(i32::from(
                        read_u1(code, &mut cursor, start)? as i8
                    )),
                ),
                "increment:s1" => operands.push(InstructionOperand::Immediate(i32::from(read_u2(
                    code,
                    &mut cursor,
                    start,
                )?
                    as i16))),
                "value:s2" | "increment:s2" => operands.push(InstructionOperand::Immediate(
                    i32::from(read_u2(code, &mut cursor, start)? as i16),
                )),
                "local:u1" if !wide => operands.push(InstructionOperand::Local(u16::from(
                    read_u1(code, &mut cursor, start)?,
                ))),
                "local:u1" => operands.push(InstructionOperand::Local(read_u2(
                    code,
                    &mut cursor,
                    start,
                )?)),
                "constant_pool:u1" => operands.push(InstructionOperand::Constant(u16::from(
                    read_u1(code, &mut cursor, start)?,
                ))),
                "constant_pool:u2" => operands.push(InstructionOperand::Constant(read_u2(
                    code,
                    &mut cursor,
                    start,
                )?)),
                "branch:s2" => operands.push(InstructionOperand::Branch(i32::from(read_u2(
                    code,
                    &mut cursor,
                    start,
                )?
                    as i16))),
                "branch:s4" => {
                    operands.push(InstructionOperand::Branch(
                        read_u4(code, &mut cursor, start)? as i32,
                    ))
                }
                "count:u1" => operands.push(InstructionOperand::Count(read_u1(
                    code,
                    &mut cursor,
                    start,
                )?)),
                "dimensions:u1" => operands.push(InstructionOperand::Dimensions(read_u1(
                    code,
                    &mut cursor,
                    start,
                )?)),
                "atype:u1" => operands.push(InstructionOperand::ArrayType(read_u1(
                    code,
                    &mut cursor,
                    start,
                )?)),
                "zero:u1" => check_zero(read_u1(code, &mut cursor, start)?, start)?,
                "zero:u2" => check_zero(read_u2(code, &mut cursor, start)?, start)?,
                other => {
                    return Err(error(
                        InstructionErrorKind::Manifest,
                        start,
                        format!("unsupported manifest operand {other}"),
                    ));
                }
            }
        }
        if let Some(InstructionOperand::Constant(index)) = operands
            .iter()
            .find(|v| matches!(v, InstructionOperand::Constant(_)))
        {
            validate_constant(pool, *index, metadata.constant_pool, start)?;
        }
        let id = InstructionId(u32::try_from(instructions.len()).map_err(|_| {
            error(
                InstructionErrorKind::Manifest,
                start,
                "too many instructions",
            )
        })?);
        let offset = u32::try_from(start).map_err(|_| {
            error(
                InstructionErrorKind::Manifest,
                start,
                "code offset exceeds u32",
            )
        })?;
        offsets.insert(offset, id);
        instructions.push(LocatedInstruction {
            id,
            offset,
            instruction: Instruction {
                opcode,
                wide,
                operands,
            },
        });
    }
    Ok(DecodedCode {
        instructions,
        offsets,
    })
}

fn check_metadata(
    metadata: &crate::OpcodeMetadata,
    major: u16,
    offset: usize,
) -> Result<(), InstructionError> {
    if metadata.width == "invalid" || metadata.operands == "invalid" {
        return Err(error(
            InstructionErrorKind::InvalidOpcode,
            offset,
            format!("opcode {} is reserved", metadata.mnemonic),
        ));
    }
    let since = metadata
        .since
        .split('.')
        .next()
        .and_then(|v| v.parse::<u16>().ok())
        .ok_or_else(|| {
            error(
                InstructionErrorKind::Manifest,
                offset,
                "invalid since version",
            )
        })?;
    if major < since {
        return Err(error(
            InstructionErrorKind::Version,
            offset,
            format!(
                "opcode {} requires classfile version {}, found {}",
                metadata.mnemonic, metadata.since, major
            ),
        ));
    }
    if metadata.until != "unbounded" {
        let until = metadata
            .until
            .split('.')
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .ok_or_else(|| {
                error(
                    InstructionErrorKind::Manifest,
                    offset,
                    "invalid until version",
                )
            })?;
        if major > until {
            return Err(error(
                InstructionErrorKind::Version,
                offset,
                format!(
                    "opcode {} ended with classfile version {}, found {}",
                    metadata.mnemonic, metadata.until, major
                ),
            ));
        }
    }
    Ok(())
}

fn validate_constant(
    pool: &ConstantPool,
    index: u16,
    category: &str,
    offset: usize,
) -> Result<(), InstructionError> {
    let value = pool.entry(index, index).map_err(|cause| {
        error(
            InstructionErrorKind::ConstantPool,
            offset,
            cause.to_string(),
        )
    })?;
    let valid = match category {
        "Fieldref" => matches!(value, Constant::Fieldref { .. }),
        "Methodref" => matches!(value, Constant::Methodref { .. }),
        "InterfaceMethodref" => matches!(value, Constant::InterfaceMethodref { .. }),
        "Methodref|InterfaceMethodref" => matches!(
            value,
            Constant::Methodref { .. } | Constant::InterfaceMethodref { .. }
        ),
        "InvokeDynamic" => matches!(value, Constant::InvokeDynamic { .. }),
        "Class" => matches!(value, Constant::Class { .. }),
        "loadable-category-1" => !matches!(value, Constant::Long(_) | Constant::Double(_)),
        "loadable-category-2" => matches!(value, Constant::Long(_) | Constant::Double(_)),
        "none" => false,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(error(
            InstructionErrorKind::ConstantPool,
            offset,
            format!("constant pool index {index} is not {category}"),
        ))
    }
}

fn read_u1(code: &[u8], cursor: &mut usize, start: usize) -> Result<u8, InstructionError> {
    let value = code.get(*cursor).copied().ok_or_else(|| {
        error(
            InstructionErrorKind::Truncated,
            start,
            "truncated instruction",
        )
    })?;
    *cursor += 1;
    Ok(value)
}
fn read_u2(code: &[u8], cursor: &mut usize, start: usize) -> Result<u16, InstructionError> {
    Ok(u16::from_be_bytes([
        read_u1(code, cursor, start)?,
        read_u1(code, cursor, start)?,
    ]))
}
fn read_u4(code: &[u8], cursor: &mut usize, start: usize) -> Result<u32, InstructionError> {
    Ok(u32::from_be_bytes([
        read_u1(code, cursor, start)?,
        read_u1(code, cursor, start)?,
        read_u1(code, cursor, start)?,
        read_u1(code, cursor, start)?,
    ]))
}
fn check_zero<T: Default + PartialEq>(value: T, start: usize) -> Result<(), InstructionError> {
    if value == T::default() {
        Ok(())
    } else {
        Err(error(
            InstructionErrorKind::ReservedByte,
            start,
            "reserved operand bytes must be zero",
        ))
    }
}
fn error(
    kind: InstructionErrorKind,
    offset: usize,
    message: impl Into<String>,
) -> InstructionError {
    InstructionError {
        kind,
        offset: u32::try_from(offset).unwrap_or(u32::MAX),
        message: message.into(),
    }
}
