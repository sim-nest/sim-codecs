use sim_codec_classfile::{
    ByteReader, ConstantPool, Instruction, InstructionErrorKind, InstructionId, LocatedInstruction,
    Opcode, decode_instructions, encode_instructions,
};

fn pool() -> ConstantPool {
    let bytes = [
        0, 7, 1, 0, 1, b'C', 7, 0, 1, 1, 0, 1, b'f', 1, 0, 1, b'I', 12, 0, 3, 0, 4, 9, 0, 2, 0, 5,
    ];
    ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 69).unwrap()
}

#[test]
fn decoded_instruction_corpus_round_trips_byte_identically() {
    let corpus = [
        vec![
            0x03, 0x10, 0xfe, 0x15, 7, 0xc4, 0x84, 0x01, 0x02, 0xff, 0xfd, 0xb2, 0, 6, 0xa7, 0xff,
            0xf2,
        ],
        table_switch_at(0),
        table_switch_at(1),
        lookup_switch_at(2),
        lookup_switch_at(3),
    ];
    for original in corpus {
        let mut decoded = decode_instructions(&original, 69, &pool()).unwrap();
        assert_eq!(encode_instructions(&mut decoded, 69).unwrap(), original);
    }
}

#[test]
fn insertion_recomputes_offsets_branches_and_edited_structure() {
    let mut edited = decode_instructions(&[0xa7, 0, 3, 0xb1], 69, &pool()).unwrap();
    edited.instructions.insert(
        1,
        LocatedInstruction {
            id: InstructionId(2),
            offset: 3,
            instruction: Instruction {
                opcode: Opcode::Nop,
                wide: false,
                operands: vec![],
            },
        },
    );
    let encoded = encode_instructions(&mut edited, 69).unwrap();
    assert_eq!(encoded, [0xa7, 0, 4, 0, 0xb1]);
    let reparsed = decode_instructions(&encoded, 69, &pool()).unwrap();
    assert_eq!(
        reparsed.offsets.keys().collect::<Vec<_>>(),
        edited.offsets.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        reparsed
            .instructions
            .iter()
            .map(|located| &located.instruction)
            .collect::<Vec<_>>(),
        edited
            .instructions
            .iter()
            .map(|located| &located.instruction)
            .collect::<Vec<_>>()
    );
}

#[test]
fn insertion_past_branch_width_is_a_located_refusal() {
    let mut edited = decode_instructions(&[0xa7, 0, 3, 0xb1], 69, &pool()).unwrap();
    let nops = (0..32_766).map(|index| LocatedInstruction {
        id: InstructionId(index + 2),
        offset: 3,
        instruction: Instruction {
            opcode: Opcode::Nop,
            wide: false,
            operands: vec![],
        },
    });
    edited.instructions.splice(1..1, nops);
    let failure = encode_instructions(&mut edited, 69).unwrap_err();
    assert_eq!(failure.kind, InstructionErrorKind::WidthOverflow);
    assert_eq!(failure.offset, 0);
    assert!(failure.message.contains("exceeds s2"));
}

fn table_switch_at(start: usize) -> Vec<u8> {
    let mut code = vec![0; start];
    code.push(Opcode::Tableswitch as u8);
    code.resize(code.len() + (4 - ((start + 1) % 4)) % 4, 0);
    code.extend_from_slice(&0i32.to_be_bytes());
    code.extend_from_slice(&(-1i32).to_be_bytes());
    code.extend_from_slice(&1i32.to_be_bytes());
    for _ in 0..3 {
        code.extend_from_slice(&0i32.to_be_bytes());
    }
    code
}

fn lookup_switch_at(start: usize) -> Vec<u8> {
    let mut code = vec![0; start];
    code.push(Opcode::Lookupswitch as u8);
    code.resize(code.len() + (4 - ((start + 1) % 4)) % 4, 0);
    code.extend_from_slice(&0i32.to_be_bytes());
    code.extend_from_slice(&2i32.to_be_bytes());
    for (key, displacement) in [(-7i32, 0i32), (42, 0)] {
        code.extend_from_slice(&key.to_be_bytes());
        code.extend_from_slice(&displacement.to_be_bytes());
    }
    code
}
