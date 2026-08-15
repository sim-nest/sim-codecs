use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::{ByteErrorKind, ByteReader, ByteWriter, decode_modified_utf8, encode_modified_utf8};
use crate::{FIXTURE_EXPECTATIONS, SCOPE};
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
