use sim_codec_classfile::{
    ByteReader, ConstantPool, ExceptionHandlerRange, InstructionErrorKind, InstructionId,
    InstructionOperand, Opcode, decode_instructions, validate_exception_handlers,
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
        0x03, 0x10, 0xfe, 0x15, 7, 0xc4, 0x84, 0x01, 0x02, 0xff, 0xfd, 0xb2, 0, 6, 0xa7, 0xff, 0xf2,
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
fn switches_decode_at_every_alignment_offset() {
    for start in 0..4usize {
        let mut table = vec![0; start];
        table.push(0xaa);
        table.resize(table.len() + (4 - ((start + 1) % 4)) % 4, 0);
        table.extend_from_slice(&0i32.to_be_bytes());
        table.extend_from_slice(&(-1i32).to_be_bytes());
        table.extend_from_slice(&1i32.to_be_bytes());
        for displacement in [0i32, 0, 0] {
            table.extend_from_slice(&displacement.to_be_bytes());
        }
        let decoded = decode_instructions(&table, 69, &pool()).unwrap();
        let switch = &decoded.instructions[start].instruction;
        assert_eq!(switch.opcode, Opcode::Tableswitch);
        assert_eq!(
            switch.operands,
            vec![
                InstructionOperand::Branch(0),
                InstructionOperand::TableLow(-1),
                InstructionOperand::TableHigh(1),
                InstructionOperand::Branch(0),
                InstructionOperand::Branch(0),
                InstructionOperand::Branch(0),
            ]
        );

        let mut lookup = vec![0; start];
        lookup.push(0xab);
        lookup.resize(lookup.len() + (4 - ((start + 1) % 4)) % 4, 0);
        lookup.extend_from_slice(&0i32.to_be_bytes());
        lookup.extend_from_slice(&2i32.to_be_bytes());
        for (key, displacement) in [(-7i32, 0i32), (42, 0)] {
            lookup.extend_from_slice(&key.to_be_bytes());
            lookup.extend_from_slice(&displacement.to_be_bytes());
        }
        let decoded = decode_instructions(&lookup, 69, &pool()).unwrap();
        assert_eq!(
            decoded.instructions[start].instruction.opcode,
            Opcode::Lookupswitch
        );
    }
}

#[test]
fn lookup_rejects_the_first_out_of_order_key_at_its_origin() {
    let mut code = vec![0xab, 0, 0, 0];
    code.extend_from_slice(&0i32.to_be_bytes());
    code.extend_from_slice(&3i32.to_be_bytes());
    for (key, displacement) in [(1i32, 0i32), (9, 0), (4, 0)] {
        code.extend_from_slice(&key.to_be_bytes());
        code.extend_from_slice(&displacement.to_be_bytes());
    }
    let error = decode_instructions(&code, 69, &pool()).unwrap_err();
    assert_eq!(error.kind, InstructionErrorKind::VariableLayout);
    assert_eq!(error.offset, 28);
    assert!(error.message.contains("out-of-order key 4 follows 9"));
}

#[test]
fn rejects_non_boundary_control_flow_and_handler_ranges() {
    let branch = decode_instructions(&[0xa7, 0, 1], 69, &pool()).unwrap_err();
    assert_eq!(branch.kind, InstructionErrorKind::InvalidTarget);
    assert_eq!(branch.offset, 1);

    let decoded = decode_instructions(&[0x00, 0xb1], 69, &pool()).unwrap();
    let handler = validate_exception_handlers(
        &decoded,
        2,
        &[ExceptionHandlerRange {
            start: 0,
            end: 1,
            handler: 2,
        }],
    )
    .unwrap_err();
    assert_eq!(handler.kind, InstructionErrorKind::InvalidHandler);
    assert_eq!(handler.offset, 2);
    assert!(handler.message.contains("exception handler 2"));
}

#[test]
fn rejects_malformed_variable_layouts_at_their_exact_origin() {
    let mut duplicate = vec![0xab, 0, 0, 0];
    duplicate.extend_from_slice(&0i32.to_be_bytes());
    duplicate.extend_from_slice(&2i32.to_be_bytes());
    for (key, displacement) in [(5i32, 0i32), (5, 0)] {
        duplicate.extend_from_slice(&key.to_be_bytes());
        duplicate.extend_from_slice(&displacement.to_be_bytes());
    }
    let error = decode_instructions(&duplicate, 69, &pool()).unwrap_err();
    assert_eq!(error.kind, InstructionErrorKind::VariableLayout);
    assert_eq!(error.offset, 20);
    assert!(error.message.contains("duplicate key 5"));

    let mut negative_count = vec![0xab, 0, 0, 0];
    negative_count.extend_from_slice(&0i32.to_be_bytes());
    negative_count.extend_from_slice(&(-1i32).to_be_bytes());
    let error = decode_instructions(&negative_count, 69, &pool()).unwrap_err();
    assert_eq!(error.kind, InstructionErrorKind::VariableLayout);
    assert_eq!(error.offset, 8);
    assert!(error.message.contains("pair count -1"));

    let bad_padding = decode_instructions(&[0xaa, 1], 69, &pool()).unwrap_err();
    assert_eq!(bad_padding.kind, InstructionErrorKind::ReservedByte);
    assert_eq!(bad_padding.offset, 1);

    let outside = decode_instructions(&[0xc8, 0x7f, 0xff, 0xff, 0xff], 69, &pool()).unwrap_err();
    assert_eq!(outside.kind, InstructionErrorKind::InvalidTarget);
    assert!(outside.message.contains("outside the code array"));
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
