use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::{FIXTURE_EXPECTATIONS, SCOPE};

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
