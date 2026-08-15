use sim_codec_classfile::{
    ByteReader, ClassShell, CodeAttribute, Constant, ConstantSlot, ShellBudget,
};
use sim_kernel::{CodecId, SourceId};

fn decode(bytes: &[u8]) -> ClassShell {
    ClassShell::decode(
        bytes,
        bytes.len() * 2,
        ShellBudget {
            interfaces: 64,
            fields: 256,
            methods: 256,
            attributes: 1_024,
            attribute_bytes: bytes.len(),
        },
        CodecId(73),
        SourceId("unknown-attribute.class".into()),
    )
    .unwrap()
}

fn utf8_index(shell: &ClassShell, expected: &str) -> u16 {
    let units = expected.encode_utf16().collect::<Vec<_>>();
    shell
        .constant_pool
        .slots()
        .iter()
        .position(|slot| {
            matches!(slot, ConstantSlot::Entry(Constant::Utf8(value)) if value.as_code_units() == units)
        })
        .unwrap() as u16
}

fn positive_with_vendor_attribute() -> Vec<u8> {
    let original = include_bytes!("../fixtures/positive.class");
    let shell = decode(original);
    let mut reader = ByteReader::new(original, original.len());
    reader.read_u4().unwrap();
    reader.read_u2().unwrap();
    reader.read_u2().unwrap();
    sim_codec_classfile::ConstantPool::decode(&mut reader, shell.major_version).unwrap();
    let pool_end = reader.offset();
    let old_count = u16::from_be_bytes([original[8], original[9]]);
    let name = b"VendorPayload";
    let mut entry = vec![1, 0, name.len() as u8];
    entry.extend_from_slice(name);
    let vendor_name_index = old_count;
    let attribute_count_offset = shell.attributes[0].origin.span.start - 2 + entry.len();

    let mut bytes = original.to_vec();
    bytes[8..10].copy_from_slice(&(old_count + 1).to_be_bytes());
    bytes.splice(pool_end..pool_end, entry);
    let count = u16::from_be_bytes([
        bytes[attribute_count_offset],
        bytes[attribute_count_offset + 1],
    ]);
    bytes[attribute_count_offset..attribute_count_offset + 2]
        .copy_from_slice(&(count + 1).to_be_bytes());
    bytes.extend_from_slice(&vendor_name_index.to_be_bytes());
    bytes.extend_from_slice(&4u32.to_be_bytes());
    bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    bytes
}

#[test]
fn untouched_unknown_attribute_classfile_is_byte_identical() {
    let bytes = include_bytes!("../fixtures/unknown-attribute.class");
    let shell = decode(bytes);
    assert_eq!(shell.encode(bytes.len()).unwrap(), bytes);

    let vendor_name = utf8_index(&shell, "FuturePayload");
    let vendor = shell
        .attributes
        .iter()
        .find(|attribute| attribute.name_index == vendor_name)
        .expect("fixture carries its vendor attribute");
    assert_eq!(vendor.declared_length as usize, vendor.bytes.len());
    assert_eq!(vendor.location.order, shell.attributes.len() - 1);
}

#[test]
fn method_body_edit_preserves_vendor_attribute_bytes_and_position() {
    let original = positive_with_vendor_attribute();
    let mut shell = decode(&original);
    let vendor_name = utf8_index(&shell, "VendorPayload");
    let before = shell
        .attributes
        .iter()
        .map(|attribute| {
            (
                attribute.name_index,
                attribute.bytes.clone(),
                attribute.location,
            )
        })
        .collect::<Vec<_>>();
    let code_name = utf8_index(&shell, "Code");
    let (method_index, code_bytes) = shell
        .methods
        .iter()
        .enumerate()
        .find_map(|(method_index, method)| {
            method
                .attributes
                .iter()
                .find(|attribute| attribute.name_index == code_name)
                .map(|attribute| (method_index, attribute.bytes.clone()))
        })
        .unwrap();
    let mut code = CodeAttribute::decode(&mut ByteReader::new(&code_bytes, code_bytes.len()))
        .unwrap()
        .code;
    let last = code.last_mut().expect("fixture method has bytecode");
    *last = if *last == 0xb1 { 0xb1 } else { *last };
    // Insert a no-op: this is a real layout edit, while the checked Code encoder validates all
    // exception boundaries before committing it.
    code.insert(0, 0x00);
    let report = shell
        .replace_method_code(method_index, code, original.len() * 2)
        .unwrap();
    assert_eq!(report.invalidated.len(), 1);
    assert!(report.invalidated[0].shifts_following_layout);

    let after = shell
        .attributes
        .iter()
        .map(|attribute| {
            (
                attribute.name_index,
                attribute.bytes.clone(),
                attribute.location,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(after, before);
    let vendor_position = after
        .iter()
        .position(|(name, _, _)| *name == vendor_name)
        .unwrap();
    assert_eq!(
        vendor_position,
        before
            .iter()
            .position(|(name, _, _)| *name == vendor_name)
            .unwrap()
    );

    let encoded = shell.encode(original.len() * 2).unwrap();
    let reparsed = decode(&encoded);
    assert_eq!(
        reparsed.attributes[vendor_position].bytes,
        before[vendor_position].1
    );
}
