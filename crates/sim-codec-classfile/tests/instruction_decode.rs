use sim_codec_classfile::{
    ByteReader, ConstantPool, InstructionErrorKind, InstructionId, InstructionOperand, Opcode,
    decode_instructions,
};

fn pool() -> ConstantPool {
    // Utf8 "C", Class #1, Utf8 "f", Utf8 "I", NameAndType #3:#4, Fieldref #2.#5.
    let bytes = [
        0, 7, 1, 0, 1, b'C', 7, 0, 1, 1, 0, 1, b'f', 1, 0, 1, b'I', 12, 0, 3, 0, 4, 9, 0, 2, 0, 5,
    ];
    ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 69).unwrap()
}

#[test]
fn common_layouts_have_stable_first_byte_offsets() {
    let code = [
        0x03, 0x10, 0xfe, 0x15, 7, 0xc4, 0x84, 0x01, 0x02, 0xff, 0xfd, 0xb2, 0, 6, 0xa7, 0xff, 0xf1,
    ];
    let decoded = decode_instructions(&code, 69, &pool()).unwrap();
    assert_eq!(decoded.instructions.len(), 6);
    for instruction in &decoded.instructions {
        assert_eq!(
            decoded.offsets.get(&instruction.offset),
            Some(&instruction.id)
        );
        assert_eq!(
            code[usize::try_from(instruction.offset).unwrap()],
            if instruction.instruction.wide {
                0xc4
            } else {
                instruction.instruction.opcode as u8
            }
        );
    }
    assert_eq!(decoded.offsets.get(&0), Some(&InstructionId(0)));
    assert_eq!(decoded.offsets.get(&5), Some(&InstructionId(3)));
    assert_eq!(decoded.instructions[3].instruction.opcode, Opcode::Iinc);
    assert_eq!(
        decoded.instructions[3].instruction.operands,
        vec![
            InstructionOperand::Local(258),
            InstructionOperand::Immediate(-3)
        ]
    );
}

#[test]
fn rejects_version_reserved_bytes_categories_and_illegal_prefix() {
    let version = decode_instructions(&[0xba, 0, 5, 0, 0], 50, &pool()).unwrap_err();
    assert_eq!(version.kind, InstructionErrorKind::Version);
    assert!(version.message.contains("51.0"));

    let reserved = decode_instructions(&[0xb9, 0, 5, 1, 1], 69, &pool()).unwrap_err();
    assert_eq!(reserved.kind, InstructionErrorKind::ReservedByte);

    let category = decode_instructions(&[0xbb, 0, 5], 69, &pool()).unwrap_err();
    assert_eq!(category.kind, InstructionErrorKind::ConstantPool);

    let prefix = decode_instructions(&[0xc4, 0x60], 69, &pool()).unwrap_err();
    assert_eq!(prefix.kind, InstructionErrorKind::IllegalWide);
}
