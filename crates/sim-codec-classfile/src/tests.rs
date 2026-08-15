use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::{
    ByteErrorKind, ByteReader, ByteWriter, Constant, ConstantPool, ConstantPoolErrorKind,
    ConstantSlot, ModuleAttribute, decode_modified_utf8, encode_modified_utf8,
};
use crate::{ClassShell, ShellBudget, ShellErrorKind};
use crate::{FIXTURE_EXPECTATIONS, SCOPE};
use sim_kernel::{CodecId, SourceId};
use sim_text::CodeUnitString;

const RETAINED_FIXTURES: &[(&str, &[u8])] = &[
    ("positive", include_bytes!("../fixtures/positive.class")),
    ("negative", include_bytes!("../fixtures/negative.class")),
    (
        "adversarial",
        include_bytes!("../fixtures/adversarial.class"),
    ),
    ("module", include_bytes!("../fixtures/module-info.class")),
    ("record", include_bytes!("../fixtures/record.class")),
    ("sealed", include_bytes!("../fixtures/sealed.class")),
    ("annotation", include_bytes!("../fixtures/annotation.class")),
    ("dynamic", include_bytes!("../fixtures/dynamic.class")),
    (
        "unknown-attribute",
        include_bytes!("../fixtures/unknown-attribute.class"),
    ),
];

#[test]
fn every_declared_case_has_retained_nonempty_bytes() {
    let expectation_names = FIXTURE_EXPECTATIONS
        .lines()
        .filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"'))
        .collect::<BTreeSet<_>>();
    let retained_names = RETAINED_FIXTURES
        .iter()
        .map(|(name, bytes)| {
            assert!(!bytes.is_empty(), "fixture {name} has no retained bytes");
            *name
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(expectation_names, retained_names);
}

#[test]
fn scope_bounds_are_data_and_expectations_freeze_identity_and_failure_location() {
    assert!(SCOPE.contains("minimum_major = 45"));
    assert!(SCOPE.contains("maximum_major = 69"));
    assert_eq!(
        FIXTURE_EXPECTATIONS
            .lines()
            .filter(|line| line.starts_with("sha256 = "))
            .count(),
        RETAINED_FIXTURES.len()
    );
    assert_eq!(
        FIXTURE_EXPECTATIONS.matches("failure_offset = ").count(),
        RETAINED_FIXTURES.len()
    );
    assert_eq!(
        FIXTURE_EXPECTATIONS.matches("failure_path = ").count(),
        RETAINED_FIXTURES.len()
    );
}

#[test]
fn retained_bytes_match_the_independently_authored_identities() {
    let document = FIXTURE_EXPECTATIONS.parse::<toml::Table>().unwrap();
    let fixtures = document["fixture"].as_array().unwrap();
    for (name, bytes) in RETAINED_FIXTURES {
        let expected = fixtures
            .iter()
            .find(|fixture| fixture["name"].as_str() == Some(name))
            .unwrap();
        let actual = format!("{:x}", Sha256::digest(bytes));
        assert_eq!(expected["sha256"].as_str(), Some(actual.as_str()), "{name}");
        assert_eq!(
            expected["expected_roundtrip_sha256"].as_str(),
            Some(actual.as_str()),
            "{name}"
        );
    }
}

#[test]
fn big_endian_lanes_and_declared_subreaders_are_exact() {
    let mut writer = ByteWriter::new(7);
    writer.write_u1(0x12).unwrap();
    writer.write_u2(0x3456).unwrap();
    writer.write_u4(0x789a_bcde).unwrap();
    let mut reader = ByteReader::new(writer.as_slice(), 7);
    assert_eq!(reader.read_u1().unwrap(), 0x12);
    let mut child = reader.sub_reader(2).unwrap();
    assert_eq!(child.read_u2().unwrap(), 0x3456);
    assert_eq!(child.remaining(), 0);
    assert_eq!(reader.read_u4().unwrap(), 0x789a_bcde);
}

#[test]
fn modified_utf8_nul_and_surrogate_pair_round_trip_exactly() {
    let nul = [0xc0, 0x80];
    let decoded = decode_modified_utf8(&nul, 2).unwrap();
    assert_eq!(decoded.as_code_units(), &[0]);
    assert_eq!(encode_modified_utf8(&decoded, 2).unwrap(), nul);

    let supplementary = [0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80];
    let decoded = decode_modified_utf8(&supplementary, 2).unwrap();
    assert_eq!(decoded.as_code_units(), &[0xd83d, 0xde00]);
    assert_eq!(encode_modified_utf8(&decoded, 6).unwrap(), supplementary);
}

#[test]
fn strict_failures_are_distinct_and_located() {
    let cases: &[(&[u8], usize, ByteErrorKind, usize)] = &[
        (&[0xc1, 0x81], 2, ByteErrorKind::OverlongModifiedUtf8, 0),
        (&[0xe1, 0x80], 2, ByteErrorKind::Truncated, 2),
        (&[0], 1, ByteErrorKind::IllegalZero, 0),
        (
            &[0xed, 0xa0, 0x80, b'a'],
            4,
            ByteErrorKind::MalformedSurrogate,
            0,
        ),
        (b"ab", 1, ByteErrorKind::BudgetExceeded, 1),
    ];
    for (bytes, budget, kind, offset) in cases {
        let error = decode_modified_utf8(bytes, *budget).unwrap_err();
        assert_eq!((error.kind, error.offset), (*kind, *offset));
    }
    let malformed = CodeUnitString::from_code_units(vec![0xd800]);
    assert_eq!(
        encode_modified_utf8(&malformed, 3).unwrap_err().kind,
        ByteErrorKind::MalformedSurrogate
    );
}

#[test]
fn reader_and_writer_budgets_fail_before_growth() {
    let reader = ByteReader::new(&[1, 2], 1);
    assert_eq!(
        reader.preflight_allocation(2).unwrap_err().kind,
        ByteErrorKind::BudgetExceeded
    );
    let mut writer = ByteWriter::new(1);
    assert_eq!(
        writer.write_u2(1).unwrap_err().kind,
        ByteErrorKind::BudgetExceeded
    );
    assert!(writer.as_slice().is_empty());
}

#[test]
fn retained_constant_pools_round_trip_byte_identically() {
    for (name, bytes) in RETAINED_FIXTURES {
        if matches!(*name, "negative" | "adversarial") {
            continue;
        }
        let mut reader = ByteReader::new(bytes, bytes.len());
        assert_eq!(reader.read_u4().unwrap(), 0xcafe_babe, "{name}");
        reader.read_u2().unwrap();
        let major = reader.read_u2().unwrap();
        let pool_start = reader.offset();
        let pool = ConstantPool::decode(&mut reader, major).unwrap();
        let mut writer = ByteWriter::new(bytes.len());
        pool.encode(&mut writer, major).unwrap();
        assert_eq!(
            writer.as_slice(),
            &bytes[pool_start..reader.offset()],
            "{name}"
        );
    }
}

#[test]
fn long_at_index_five_preserves_explicit_sixth_slot_and_exact_bytes() {
    let bytes = [
        0, 7, // constant_pool_count
        3, 0, 0, 0, 1, // #1 Integer
        3, 0, 0, 0, 2, // #2 Integer
        3, 0, 0, 0, 3, // #3 Integer
        3, 0, 0, 0, 4, // #4 Integer
        5, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, // #5 Long, #6 unusable
    ];
    let mut reader = ByteReader::new(&bytes, bytes.len());
    let pool = ConstantPool::decode(&mut reader, 69).unwrap();
    assert!(matches!(pool.slots()[6], ConstantSlot::Unusable));
    let mut writer = ByteWriter::new(bytes.len());
    pool.encode(&mut writer, 69).unwrap();
    assert_eq!(writer.as_slice(), bytes);
}

#[test]
fn reference_to_two_slot_hole_names_source_and_target_indices() {
    let bytes = [
        0, 7, // constant_pool_count
        7, 0, 6, // #1 Class illegally points at #6
        3, 0, 0, 0, 2, // #2 Integer
        3, 0, 0, 0, 3, // #3 Integer
        3, 0, 0, 0, 4, // #4 Integer
        5, 0, 0, 0, 0, 0, 0, 0, 5, // #5 Long, #6 unusable
    ];
    let error = ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 69).unwrap_err();
    assert_eq!(error.kind, ConstantPoolErrorKind::UnusableTarget);
    assert_eq!((error.index, error.target_index), (1, Some(6)));
    assert!(
        error
            .to_string()
            .contains("index 1 points at unusable index 6")
    );
}

#[test]
fn constant_version_bounds_are_located() {
    let bytes = [0, 2, 19, 0, 1];
    let error = ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 52).unwrap_err();
    assert_eq!(error.kind, ConstantPoolErrorKind::Version);
    assert_eq!(error.index, 1);
}

fn fixture_shell(bytes: &[u8]) -> ClassShell {
    ClassShell::decode(
        bytes,
        bytes.len(),
        ShellBudget {
            interfaces: 64,
            fields: 256,
            methods: 256,
            attributes: 1_024,
            attribute_bytes: bytes.len(),
        },
        CodecId(73),
        SourceId("fixture.class".into()),
    )
    .unwrap()
}

#[test]
fn invalid_interface_index_is_structural_data_until_validation() {
    let mut shell = fixture_shell(include_bytes!("../fixtures/positive.class"));
    shell.interfaces.push(u16::MAX);
    let error = shell.validate().unwrap_err();
    assert_eq!(error.kind, ShellErrorKind::InvalidIndex);
    assert_eq!(error.index, Some(u16::MAX));
    assert_eq!(error.path, "interfaces[0]");
    assert!(error.message.contains("65535"));
}

#[test]
fn method_attribute_order_and_declaration_origins_are_exact() {
    let bytes = vec![
        0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52, 0, 9, 1, 0, 1, b'C', 7, 0, 1, 1, 0, 16, b'j', b'a',
        b'v', b'a', b'/', b'l', b'a', b'n', b'g', b'/', b'O', b'b', b'j', b'e', b'c', b't', 7, 0,
        3, 1, 0, 1, b'm', 1, 0, 3, b'(', b')', b'V', 1, 0, 1, b'A', 1, 0, 1, b'B', 0, 0x21, 0, 2,
        0, 4, 0, 0, 0, 0, 0, 1, 0, 1, 0, 5, 0, 6, 0, 2, 0, 7, 0, 0, 0, 1, 0xaa, 0, 8, 0, 0, 0, 2,
        0xbb, 0xcc, 0, 0,
    ];
    let shell = fixture_shell(&bytes);
    shell.validate().unwrap();
    let method = &shell.methods[0];
    assert_eq!(
        method
            .attributes
            .iter()
            .map(|attribute| attribute.name_index)
            .collect::<Vec<_>>(),
        [7, 8]
    );
    assert_eq!(method.attributes[0].bytes, [0xaa]);
    assert_eq!(method.attributes[1].bytes, [0xbb, 0xcc]);
    assert!(method.attributes[0].origin.span.end <= method.attributes[1].origin.span.start);
    assert!(method.origin.span.start < method.origin.span.end);
    assert_eq!(method.origin.source.0, "fixture.class");
    assert_eq!(
        shell.origin.span,
        sim_kernel::Span {
            start: 0,
            end: bytes.len()
        }
    );
}

#[test]
fn aggregate_attribute_budget_fails_before_retaining_excess() {
    let bytes = include_bytes!("../fixtures/positive.class");
    let error = ClassShell::decode(
        bytes,
        bytes.len(),
        ShellBudget {
            interfaces: 64,
            fields: 256,
            methods: 256,
            attributes: 1_024,
            attribute_bytes: 0,
        },
        CodecId(73),
        SourceId("fixture.class".into()),
    )
    .unwrap_err();
    assert_eq!(error.kind, ShellErrorKind::Budget);
}

#[test]
fn module_info_structured_payload_round_trips_byte_identically() {
    let shell = fixture_shell(include_bytes!("../fixtures/module-info.class"));
    let module_name_index = shell
        .constant_pool
        .slots()
        .iter()
        .position(|slot| match slot {
            ConstantSlot::Entry(Constant::Utf8(value)) => {
                value.as_code_units() == "Module".encode_utf16().collect::<Vec<_>>()
            }
            _ => false,
        })
        .expect("module-info fixture has a Module attribute") as u16;
    let payload = &shell
        .attributes
        .iter()
        .find(|attribute| attribute.name_index == module_name_index)
        .expect("module-info fixture has class-level Module metadata")
        .bytes;

    let module = ModuleAttribute::decode(&mut ByteReader::new(payload, payload.len())).unwrap();
    assert_eq!(module.encode(payload.len()).unwrap(), *payload);
}

#[test]
fn standard_attribute_version_table_covers_modern_metadata() {
    assert_eq!(crate::standard_attribute_min_major("Module"), Some(53));
    assert_eq!(crate::standard_attribute_min_major("NestMembers"), Some(55));
    assert_eq!(crate::standard_attribute_min_major("Record"), Some(60));
    assert_eq!(
        crate::standard_attribute_min_major("PermittedSubclasses"),
        Some(61)
    );
    assert_eq!(crate::standard_attribute_min_major("VendorAttribute"), None);
}
