//! Manifest scalar-width calculation for the checked layout pass.

use crate::instruction::error;
use crate::{Instruction, InstructionError, InstructionErrorKind};

pub(super) fn fixed_operand_width(
    instruction: &Instruction,
    start: usize,
) -> Result<usize, InstructionError> {
    let mut width = 0usize;
    for field in instruction.opcode.metadata().operands.split(',') {
        let field_width = match field {
            "none" => 0,
            "value:s1" | "constant_pool:u1" | "count:u1" | "dimensions:u1" | "atype:u1"
            | "zero:u1" => 1,
            "local:u1" if !instruction.wide => 1,
            "local:u1" => 2,
            "increment:s1" if !instruction.wide => 1,
            "increment:s1" | "value:s2" | "increment:s2" | "constant_pool:u2" | "branch:s2"
            | "zero:u2" => 2,
            "branch:s4" => 4,
            other => {
                return Err(error(
                    InstructionErrorKind::InvalidOperands,
                    start,
                    format!("unsupported manifest operand {other}"),
                ));
            }
        };
        width = width.checked_add(field_width).ok_or_else(|| {
            error(
                InstructionErrorKind::WidthOverflow,
                start,
                "operand width overflows",
            )
        })?;
    }
    Ok(width)
}
