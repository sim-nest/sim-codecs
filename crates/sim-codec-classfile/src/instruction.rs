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
    /// The lowest key accepted by a `tableswitch`.
    TableLow(i32),
    /// The highest key accepted by a `tableswitch`.
    TableHigh(i32),
    /// One key in a `lookupswitch`, followed by its branch operand.
    LookupKey(i32),
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

/// Raw Code-attribute exception-table offsets to validate against decoded instructions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExceptionHandlerRange {
    /// First protected instruction, inclusive.
    pub start: u16,
    /// End of the protected range, exclusive; the code length is permitted.
    pub end: u16,
    /// First instruction of the handler.
    pub handler: u16,
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
    /// A variable layout is malformed or too large to represent safely.
    VariableLayout,
    /// A relative control-flow target is outside the code array or not an instruction boundary.
    InvalidTarget,
    /// An exception handler range is empty, reversed, or not instruction-aligned.
    InvalidHandler,
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

/// Decode JVM instructions using generated metadata plus the three irregular layouts.
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
        let operands = if opcode == Opcode::Tableswitch {
            decode_table_switch(code, &mut cursor, start)?
        } else if opcode == Opcode::Lookupswitch {
            decode_lookup_switch(code, &mut cursor, start)?
        } else {
            if metadata.width == "variable" && !wide {
                return Err(error(
                    InstructionErrorKind::VariableLayout,
                    start,
                    format!("unsupported variable layout for {}", metadata.mnemonic),
                ));
            }
            let mut operands = Vec::new();
            for field in metadata.operands.split(',') {
                match field {
                    "none" => {}
                    "value:s1" | "increment:s1" if !wide => operands.push(
                        InstructionOperand::Immediate(i32::from(
                            read_u1(code, &mut cursor, start)? as i8,
                        )),
                    ),
                    "increment:s1" => operands.push(InstructionOperand::Immediate(i32::from(
                        read_u2(code, &mut cursor, start)? as i16,
                    ))),
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
            operands
        };
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
    let decoded = DecodedCode {
        instructions,
        offsets,
    };
    validate_branch_targets(&decoded, code.len())?;
    Ok(decoded)
}

/// Validate Code-attribute exception ranges against an already decoded code array.
pub fn validate_exception_handlers(
    decoded: &DecodedCode,
    code_length: usize,
    handlers: &[ExceptionHandlerRange],
) -> Result<(), InstructionError> {
    for range in handlers {
        let start = usize::from(range.start);
        let end = usize::from(range.end);
        let handler = usize::from(range.handler);
        if start >= end {
            return Err(error(
                InstructionErrorKind::InvalidHandler,
                start,
                format!("exception handler range {start}..{end} is empty or reversed"),
            ));
        }
        require_boundary(decoded, start, code_length, false, "exception range start")?;
        require_boundary(decoded, end, code_length, true, "exception range end")?;
        require_boundary(decoded, handler, code_length, false, "exception handler")?;
    }
    Ok(())
}

fn decode_table_switch(
    code: &[u8],
    cursor: &mut usize,
    start: usize,
) -> Result<Vec<InstructionOperand>, InstructionError> {
    read_padding(code, cursor, start)?;
    let default = read_i4(code, cursor, start)?;
    let low = read_i4(code, cursor, start)?;
    let high = read_i4(code, cursor, start)?;
    if high < low {
        return Err(error(
            InstructionErrorKind::VariableLayout,
            start,
            format!("tableswitch high key {high} precedes low key {low}"),
        ));
    }
    let count = i64::from(high) - i64::from(low) + 1;
    let count = usize::try_from(count).map_err(|_| {
        error(
            InstructionErrorKind::VariableLayout,
            start,
            "tableswitch key range overflows addressable input",
        )
    })?;
    ensure_entries_fit(code, *cursor, count, 4, start, "tableswitch")?;
    let mut operands = Vec::with_capacity(count.saturating_add(3));
    operands.push(InstructionOperand::Branch(default));
    operands.push(InstructionOperand::TableLow(low));
    operands.push(InstructionOperand::TableHigh(high));
    for _ in 0..count {
        operands.push(InstructionOperand::Branch(read_i4(code, cursor, start)?));
    }
    Ok(operands)
}

fn decode_lookup_switch(
    code: &[u8],
    cursor: &mut usize,
    start: usize,
) -> Result<Vec<InstructionOperand>, InstructionError> {
    read_padding(code, cursor, start)?;
    let default = read_i4(code, cursor, start)?;
    let pairs_origin = *cursor;
    let pair_count = read_i4(code, cursor, start)?;
    let pair_count = usize::try_from(pair_count).map_err(|_| {
        error(
            InstructionErrorKind::VariableLayout,
            pairs_origin,
            format!("lookupswitch pair count {pair_count} is negative"),
        )
    })?;
    ensure_entries_fit(code, *cursor, pair_count, 8, start, "lookupswitch")?;
    let mut operands = Vec::with_capacity(pair_count.saturating_mul(2).saturating_add(1));
    operands.push(InstructionOperand::Branch(default));
    let mut previous = None;
    for _ in 0..pair_count {
        let key_origin = *cursor;
        let key = read_i4(code, cursor, start)?;
        if let Some(prior) = previous
            && key <= prior
        {
            let relation = if key == prior {
                "duplicate"
            } else {
                "out-of-order"
            };
            return Err(error(
                InstructionErrorKind::VariableLayout,
                key_origin,
                format!("lookupswitch {relation} key {key} follows {prior}"),
            ));
        }
        let displacement = read_i4(code, cursor, start)?;
        operands.push(InstructionOperand::LookupKey(key));
        operands.push(InstructionOperand::Branch(displacement));
        previous = Some(key);
    }
    Ok(operands)
}

fn read_padding(code: &[u8], cursor: &mut usize, start: usize) -> Result<(), InstructionError> {
    let padding = (4 - (*cursor % 4)) % 4;
    for _ in 0..padding {
        let origin = *cursor;
        if read_u1(code, cursor, start)? != 0 {
            return Err(error(
                InstructionErrorKind::ReservedByte,
                origin,
                "switch alignment padding must be zero",
            ));
        }
    }
    Ok(())
}

fn ensure_entries_fit(
    code: &[u8],
    cursor: usize,
    count: usize,
    width: usize,
    start: usize,
    layout: &str,
) -> Result<(), InstructionError> {
    let bytes = count.checked_mul(width).ok_or_else(|| {
        error(
            InstructionErrorKind::VariableLayout,
            start,
            format!("{layout} entry byte count overflows"),
        )
    })?;
    let end = cursor.checked_add(bytes).ok_or_else(|| {
        error(
            InstructionErrorKind::VariableLayout,
            start,
            format!("{layout} end offset overflows"),
        )
    })?;
    if end > code.len() {
        return Err(error(
            InstructionErrorKind::Truncated,
            start,
            format!("truncated {layout}"),
        ));
    }
    Ok(())
}

fn validate_branch_targets(
    decoded: &DecodedCode,
    code_length: usize,
) -> Result<(), InstructionError> {
    for located in &decoded.instructions {
        for operand in &located.instruction.operands {
            if let InstructionOperand::Branch(displacement) = operand {
                let target = i64::from(located.offset) + i64::from(*displacement);
                let target = usize::try_from(target).map_err(|_| {
                    error(
                        InstructionErrorKind::InvalidTarget,
                        located.offset as usize,
                        format!("branch target {target} is outside the code array"),
                    )
                })?;
                require_boundary(decoded, target, code_length, false, "branch target")?;
            }
        }
    }
    Ok(())
}

fn require_boundary(
    decoded: &DecodedCode,
    offset: usize,
    code_length: usize,
    allow_end: bool,
    subject: &str,
) -> Result<(), InstructionError> {
    let offset_u32 = u32::try_from(offset).map_err(|_| {
        error(
            InstructionErrorKind::InvalidTarget,
            offset,
            format!("{subject} {offset} exceeds the classfile offset range"),
        )
    })?;
    if (allow_end && offset == code_length) || decoded.offsets.contains_key(&offset_u32) {
        return Ok(());
    }
    if offset >= code_length {
        return Err(error(
            if subject.starts_with("exception") {
                InstructionErrorKind::InvalidHandler
            } else {
                InstructionErrorKind::InvalidTarget
            },
            offset,
            format!("{subject} {offset} is outside the code array"),
        ));
    }
    Err(error(
        if subject.starts_with("exception") {
            InstructionErrorKind::InvalidHandler
        } else {
            InstructionErrorKind::InvalidTarget
        },
        offset,
        format!("{subject} {offset} is not an instruction boundary"),
    ))
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
fn read_i4(code: &[u8], cursor: &mut usize, start: usize) -> Result<i32, InstructionError> {
    Ok(read_u4(code, cursor, start)? as i32)
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
