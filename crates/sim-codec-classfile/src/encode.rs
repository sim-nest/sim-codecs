//! Checked, manifest-driven JVM instruction encoding.

use std::collections::{BTreeMap, BTreeSet};

use crate::instruction::{check_metadata, error};
use crate::{
    DecodedCode, InstructionError, InstructionErrorKind, InstructionId, InstructionOperand, Opcode,
};

mod scalar;
use scalar::fixed_operand_width;

/// Encode a decoded instruction stream after recomputing offsets, switch padding, and branches.
///
/// Branch operands are resolved through `code.offsets` before layout. This preserves their stable
/// instruction targets when callers insert, remove, or resize instructions. On success the
/// locations and offset map in `code` describe the returned bytes.
pub fn encode_instructions(
    code: &mut DecodedCode,
    major_version: u16,
) -> Result<Vec<u8>, InstructionError> {
    let targets = resolve_branch_targets(code)?;
    let offsets = layout(code, major_version)?;
    let target_offsets: BTreeMap<_, _> = code
        .instructions
        .iter()
        .zip(&offsets)
        .map(|(located, offset)| (located.id, *offset))
        .collect();
    let total = code
        .instructions
        .iter()
        .zip(&offsets)
        .try_fold(0usize, |_, (located, offset)| {
            instruction_end(&located.instruction, *offset as usize)
        })?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(total).map_err(|cause| {
        error(
            InstructionErrorKind::WidthOverflow,
            total,
            format!("cannot allocate encoded code array: {cause}"),
        )
    })?;

    for (index, located) in code.instructions.iter().enumerate() {
        let start = offsets[index] as usize;
        debug_assert_eq!(bytes.len(), start);
        encode_one(
            &mut bytes,
            &located.instruction,
            start,
            index,
            &targets,
            &target_offsets,
        )?;
    }

    for (instruction_index, located) in code.instructions.iter_mut().enumerate() {
        let start = offsets[instruction_index] as usize;
        for (operand_index, operand) in located.instruction.operands.iter_mut().enumerate() {
            if matches!(operand, InstructionOperand::Branch(_)) {
                *operand = InstructionOperand::Branch(displacement(
                    start,
                    instruction_index,
                    operand_index,
                    &targets,
                    &target_offsets,
                )?);
            }
        }
    }

    let mut rebuilt = BTreeMap::new();
    for (located, offset) in code.instructions.iter_mut().zip(offsets) {
        located.offset = offset;
        if rebuilt.insert(offset, located.id).is_some() {
            return Err(error(
                InstructionErrorKind::Manifest,
                offset as usize,
                "duplicate encoded instruction offset",
            ));
        }
    }
    code.offsets = rebuilt;
    Ok(bytes)
}

fn resolve_branch_targets(
    code: &DecodedCode,
) -> Result<BTreeMap<(usize, usize), InstructionId>, InstructionError> {
    let ids: BTreeSet<_> = code.instructions.iter().map(|located| located.id).collect();
    if ids.len() != code.instructions.len() {
        return Err(error(
            InstructionErrorKind::InvalidOperands,
            0,
            "instruction ids must be unique",
        ));
    }
    let mut targets = BTreeMap::new();
    for (instruction_index, located) in code.instructions.iter().enumerate() {
        for (operand_index, operand) in located.instruction.operands.iter().enumerate() {
            if let InstructionOperand::Branch(displacement) = operand {
                let target = i64::from(located.offset) + i64::from(*displacement);
                let target = u32::try_from(target).map_err(|_| {
                    error(
                        InstructionErrorKind::InvalidTarget,
                        located.offset as usize,
                        format!("branch target {target} is outside the original code array"),
                    )
                })?;
                let id = code.offsets.get(&target).copied().ok_or_else(|| {
                    error(
                        InstructionErrorKind::InvalidTarget,
                        located.offset as usize,
                        format!("branch target {target} is not an instruction boundary"),
                    )
                })?;
                if !ids.contains(&id) {
                    return Err(error(
                        InstructionErrorKind::InvalidTarget,
                        located.offset as usize,
                        format!("branch target instruction {:?} is absent", id),
                    ));
                }
                targets.insert((instruction_index, operand_index), id);
            }
        }
    }
    Ok(targets)
}

fn layout(code: &DecodedCode, major: u16) -> Result<Vec<u32>, InstructionError> {
    let mut offsets = Vec::with_capacity(code.instructions.len());
    let mut cursor = 0usize;
    for located in &code.instructions {
        check_metadata(located.instruction.opcode.metadata(), major, cursor)?;
        validate_shape(&located.instruction, cursor)?;
        offsets.push(u32::try_from(cursor).map_err(|_| {
            error(
                InstructionErrorKind::WidthOverflow,
                cursor,
                "encoded instruction offset exceeds u32",
            )
        })?);
        cursor = instruction_end(&located.instruction, cursor)?;
    }
    u32::try_from(cursor).map_err(|_| {
        error(
            InstructionErrorKind::WidthOverflow,
            cursor,
            "encoded code array exceeds u32",
        )
    })?;
    Ok(offsets)
}

fn instruction_end(
    instruction: &crate::Instruction,
    start: usize,
) -> Result<usize, InstructionError> {
    let body = if instruction.opcode == Opcode::Tableswitch {
        let count = instruction.operands.len().checked_sub(3).ok_or_else(|| {
            invalid(
                start,
                "tableswitch requires default, low, and high operands",
            )
        })?;
        padding(start).checked_add(12).and_then(|size| {
            count
                .checked_mul(4)
                .and_then(|entries| size.checked_add(entries))
        })
    } else if instruction.opcode == Opcode::Lookupswitch {
        let tail = instruction
            .operands
            .len()
            .checked_sub(1)
            .ok_or_else(|| invalid(start, "lookupswitch requires a default operand"))?;
        let pairs = tail / 2;
        padding(start).checked_add(8).and_then(|size| {
            pairs
                .checked_mul(8)
                .and_then(|entries| size.checked_add(entries))
        })
    } else {
        fixed_operand_width(instruction, start).map(Some)?
    }
    .ok_or_else(|| {
        error(
            InstructionErrorKind::WidthOverflow,
            start,
            "instruction width overflows",
        )
    })?;
    start
        .checked_add(1 + usize::from(instruction.wide))
        .and_then(|value| value.checked_add(body))
        .ok_or_else(|| {
            error(
                InstructionErrorKind::WidthOverflow,
                start,
                "code offset overflows",
            )
        })
}

fn validate_shape(instruction: &crate::Instruction, start: usize) -> Result<(), InstructionError> {
    let metadata = instruction.opcode.metadata();
    if instruction.wide
        && metadata.operands != "local:u1"
        && metadata.operands != "local:u1,increment:s1"
    {
        return Err(error(
            InstructionErrorKind::IllegalWide,
            start,
            format!("wide cannot modify {}", metadata.mnemonic),
        ));
    }
    if instruction.opcode == Opcode::Tableswitch {
        match instruction.operands.as_slice() {
            [
                InstructionOperand::Branch(_),
                InstructionOperand::TableLow(low),
                InstructionOperand::TableHigh(high),
                branches @ ..,
            ] if high >= low
                && i64::from(*high) - i64::from(*low) + 1 == branches.len() as i64
                && branches
                    .iter()
                    .all(|operand| matches!(operand, InstructionOperand::Branch(_))) =>
            {
                Ok(())
            }
            _ => Err(invalid(
                start,
                "tableswitch operands do not match its key range",
            )),
        }
    } else if instruction.opcode == Opcode::Lookupswitch {
        let Some((InstructionOperand::Branch(_), tail)) = instruction.operands.split_first() else {
            return Err(invalid(start, "lookupswitch requires a default branch"));
        };
        if tail.len() % 2 != 0 {
            return Err(invalid(start, "lookupswitch requires key/branch pairs"));
        }
        let mut previous = None;
        for pair in tail.chunks_exact(2) {
            let (InstructionOperand::LookupKey(key), InstructionOperand::Branch(_)) =
                (pair[0], pair[1])
            else {
                return Err(invalid(start, "lookupswitch requires key/branch pairs"));
            };
            if previous.is_some_and(|prior| key <= prior) {
                return Err(invalid(
                    start,
                    format!("lookupswitch key {key} is not strictly increasing"),
                ));
            }
            previous = Some(key);
        }
        Ok(())
    } else {
        let expected = metadata
            .operands
            .split(',')
            .filter(|field| !field.starts_with("zero:") && *field != "none")
            .count();
        if instruction.operands.len() != expected {
            return Err(invalid(
                start,
                format!(
                    "{} expects {expected} operands, found {}",
                    metadata.mnemonic,
                    instruction.operands.len()
                ),
            ));
        }
        Ok(())
    }
}

fn encode_one(
    bytes: &mut Vec<u8>,
    instruction: &crate::Instruction,
    start: usize,
    instruction_index: usize,
    targets: &BTreeMap<(usize, usize), InstructionId>,
    target_offsets: &BTreeMap<InstructionId, u32>,
) -> Result<(), InstructionError> {
    if instruction.wide {
        bytes.push(Opcode::Wide as u8);
    }
    bytes.push(instruction.opcode as u8);
    if instruction.opcode == Opcode::Tableswitch || instruction.opcode == Opcode::Lookupswitch {
        bytes.resize(bytes.len() + padding(start), 0);
        encode_switch(
            bytes,
            instruction,
            start,
            instruction_index,
            targets,
            target_offsets,
        )?;
        return Ok(());
    }
    let mut operand_index = 0usize;
    for field in instruction.opcode.metadata().operands.split(',') {
        if field == "none" {
            continue;
        }
        if field == "zero:u1" {
            bytes.push(0);
            continue;
        }
        if field == "zero:u2" {
            bytes.extend_from_slice(&[0, 0]);
            continue;
        }
        let operand = instruction.operands[operand_index];
        match (field, operand) {
            ("value:s1" | "increment:s1", InstructionOperand::Immediate(value))
                if !instruction.wide =>
            {
                push_i1(bytes, value, start)?
            }
            ("increment:s1", InstructionOperand::Immediate(value))
            | ("value:s2" | "increment:s2", InstructionOperand::Immediate(value)) => {
                push_i2(bytes, value, start)?
            }
            ("local:u1", InstructionOperand::Local(value)) if !instruction.wide => {
                push_u1(bytes, value, start)?
            }
            ("local:u1", InstructionOperand::Local(value)) => {
                bytes.extend_from_slice(&value.to_be_bytes())
            }
            ("constant_pool:u1", InstructionOperand::Constant(value)) => {
                push_u1(bytes, value, start)?
            }
            ("constant_pool:u2", InstructionOperand::Constant(value)) => {
                bytes.extend_from_slice(&value.to_be_bytes())
            }
            ("branch:s2", InstructionOperand::Branch(_)) => push_i2(
                bytes,
                displacement(
                    start,
                    instruction_index,
                    operand_index,
                    targets,
                    target_offsets,
                )?,
                start,
            )?,
            ("branch:s4", InstructionOperand::Branch(_)) => bytes.extend_from_slice(
                &displacement(
                    start,
                    instruction_index,
                    operand_index,
                    targets,
                    target_offsets,
                )?
                .to_be_bytes(),
            ),
            ("count:u1", InstructionOperand::Count(value))
            | ("dimensions:u1", InstructionOperand::Dimensions(value))
            | ("atype:u1", InstructionOperand::ArrayType(value)) => bytes.push(value),
            _ => {
                return Err(invalid(
                    start,
                    format!("operand {operand_index} does not match manifest field {field}"),
                ));
            }
        }
        operand_index += 1;
    }
    Ok(())
}

fn encode_switch(
    bytes: &mut Vec<u8>,
    instruction: &crate::Instruction,
    start: usize,
    instruction_index: usize,
    targets: &BTreeMap<(usize, usize), InstructionId>,
    target_offsets: &BTreeMap<InstructionId, u32>,
) -> Result<(), InstructionError> {
    bytes.extend_from_slice(
        &displacement(start, instruction_index, 0, targets, target_offsets)?.to_be_bytes(),
    );
    if instruction.opcode == Opcode::Tableswitch {
        let InstructionOperand::TableLow(low) = instruction.operands[1] else {
            unreachable!()
        };
        let InstructionOperand::TableHigh(high) = instruction.operands[2] else {
            unreachable!()
        };
        bytes.extend_from_slice(&low.to_be_bytes());
        bytes.extend_from_slice(&high.to_be_bytes());
        for operand_index in 3..instruction.operands.len() {
            bytes.extend_from_slice(
                &displacement(
                    start,
                    instruction_index,
                    operand_index,
                    targets,
                    target_offsets,
                )?
                .to_be_bytes(),
            );
        }
    } else {
        let pairs = (instruction.operands.len() - 1) / 2;
        bytes.extend_from_slice(&(pairs as i32).to_be_bytes());
        for operand_index in (1..instruction.operands.len()).step_by(2) {
            let InstructionOperand::LookupKey(key) = instruction.operands[operand_index] else {
                unreachable!()
            };
            bytes.extend_from_slice(&key.to_be_bytes());
            bytes.extend_from_slice(
                &displacement(
                    start,
                    instruction_index,
                    operand_index + 1,
                    targets,
                    target_offsets,
                )?
                .to_be_bytes(),
            );
        }
    }
    Ok(())
}

fn displacement(
    start: usize,
    instruction_index: usize,
    operand_index: usize,
    targets: &BTreeMap<(usize, usize), InstructionId>,
    target_offsets: &BTreeMap<InstructionId, u32>,
) -> Result<i32, InstructionError> {
    let target = targets
        .get(&(instruction_index, operand_index))
        .expect("validated branch target");
    let target_offset = target_offsets.get(target).ok_or_else(|| {
        error(
            InstructionErrorKind::InvalidTarget,
            start,
            format!("branch target {:?} is absent", target),
        )
    })?;
    i32::try_from(i64::from(*target_offset) - start as i64).map_err(|_| {
        error(
            InstructionErrorKind::WidthOverflow,
            start,
            "branch displacement exceeds s4",
        )
    })
}

fn padding(start: usize) -> usize {
    (4 - ((start + 1) % 4)) % 4
}
fn push_u1(bytes: &mut Vec<u8>, value: u16, start: usize) -> Result<(), InstructionError> {
    bytes.push(u8::try_from(value).map_err(|_| {
        error(
            InstructionErrorKind::WidthOverflow,
            start,
            format!("unsigned operand {value} exceeds u1"),
        )
    })?);
    Ok(())
}
fn push_i1(bytes: &mut Vec<u8>, value: i32, start: usize) -> Result<(), InstructionError> {
    bytes.push(i8::try_from(value).map_err(|_| {
        error(
            InstructionErrorKind::WidthOverflow,
            start,
            format!("signed operand {value} exceeds s1"),
        )
    })? as u8);
    Ok(())
}
fn push_i2(bytes: &mut Vec<u8>, value: i32, start: usize) -> Result<(), InstructionError> {
    bytes.extend_from_slice(
        &i16::try_from(value)
            .map_err(|_| {
                error(
                    InstructionErrorKind::WidthOverflow,
                    start,
                    format!("signed operand {value} exceeds s2"),
                )
            })?
            .to_be_bytes(),
    );
    Ok(())
}
fn invalid(offset: usize, message: impl Into<String>) -> InstructionError {
    error(InstructionErrorKind::InvalidOperands, offset, message)
}
