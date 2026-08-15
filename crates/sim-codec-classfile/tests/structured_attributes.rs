use sim_codec_classfile::{
    AttributeOrigin, BootstrapMethodsAttribute, ByteReader, CodeAttribute, CodeException,
    NestedAttribute, NestedAttributeOwner, StackMapFrame, StackMapTableAttribute, VerificationType,
};

#[test]
fn compressed_stack_map_frames_round_trip_without_expansion() {
    // same_locals_1_stack_item_frame(offset_delta = 6, uninitialized(offset = 0x1234)),
    // followed by append_frame(offset_delta = 9, one Object local).
    let bytes = [0, 2, 70, 8, 0x12, 0x34, 252, 0, 9, 7, 0, 21];
    let table = StackMapTableAttribute::decode(&mut ByteReader::new(&bytes, bytes.len())).unwrap();
    assert_eq!(
        table.frames,
        vec![
            StackMapFrame::SameLocalsOneStack {
                frame_type: 70,
                stack: VerificationType::Uninitialized(0x1234),
            },
            StackMapFrame::Append {
                frame_type: 252,
                offset_delta: 9,
                locals: vec![VerificationType::Object(21)],
            },
        ]
    );
    assert_eq!(table.encode(bytes.len()).unwrap(), bytes);
}

#[test]
fn code_preserves_exception_and_nested_attribute_order() {
    let code = CodeAttribute {
        max_stack: 2,
        max_locals: 1,
        code: vec![0x03, 0xac, 0xbf],
        exception_table: vec![
            CodeException {
                start_pc: 0,
                end_pc: 2,
                handler_pc: 2,
                catch_type: 41,
            },
            CodeException {
                start_pc: 1,
                end_pc: 2,
                handler_pc: 2,
                catch_type: 0,
            },
        ],
        attributes: vec![
            NestedAttribute {
                name_index: 9,
                owner: NestedAttributeOwner::Code,
                order: 0,
                declared_length: 2,
                bytes: vec![1, 2],
                origin: AttributeOrigin { start: 31, end: 39 },
            },
            NestedAttribute {
                name_index: 7,
                owner: NestedAttributeOwner::Code,
                order: 1,
                declared_length: 0,
                bytes: vec![],
                origin: AttributeOrigin { start: 39, end: 45 },
            },
        ],
    };
    let bytes = code.encode(64).unwrap();
    assert_eq!(
        CodeAttribute::decode(&mut ByteReader::new(&bytes, 64)).unwrap(),
        code
    );
}

#[test]
fn bootstrap_arguments_preserve_order_and_arity_exactly() {
    let bytes = [0, 2, 0, 17, 0, 4, 0, 31, 0, 7, 0, 31, 0, 2, 0, 18, 0, 0];
    let methods =
        BootstrapMethodsAttribute::decode(&mut ByteReader::new(&bytes, bytes.len())).unwrap();
    assert_eq!(methods.methods[0].method_ref, 17);
    assert_eq!(methods.methods[0].arguments, [31, 7, 31, 2]);
    assert_eq!(methods.methods[1].method_ref, 18);
    assert!(methods.methods[1].arguments.is_empty());
    assert_eq!(methods.encode(bytes.len()).unwrap(), bytes);
}
