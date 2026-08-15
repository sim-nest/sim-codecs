use std::panic::{AssertUnwindSafe, catch_unwind};

use sha2::{Digest, Sha256};
use sim_codec_classfile::{ByteReader, ClassShell, CodeAttribute, ShellBudget, ShellError};
use sim_kernel::{CodecId, SourceId};

const MIN_MAJOR: u16 = 45;
const MAX_MAJOR: u16 = 69;

const FIXTURES: &[(&str, &[u8])] = &[
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

fn budget(bytes: &[u8]) -> ShellBudget {
    ShellBudget {
        interfaces: bytes.len() / 2,
        fields: bytes.len() / 8,
        methods: bytes.len() / 8,
        attributes: bytes.len() / 6,
        attribute_bytes: bytes.len(),
    }
}

fn decode(bytes: &[u8], allocation_budget: usize, label: &str) -> Result<ClassShell, ShellError> {
    ClassShell::decode(
        bytes,
        allocation_budget,
        budget(bytes),
        CodecId(73),
        SourceId(label.into()),
    )
}

fn assert_located(error: &ShellError, input_len: usize) {
    assert!(!error.path.is_empty(), "unlocated error: {error:?}");
    assert!(
        error.offset <= input_len,
        "error points beyond its input: {error:?}, input length {input_len}"
    );
    assert!(!error.message.is_empty(), "undocumented error: {error:?}");
}

fn expectation(name: &str) -> toml::Table {
    let document = sim_codec_classfile::FIXTURE_EXPECTATIONS
        .parse::<toml::Table>()
        .unwrap();
    document["fixture"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"].as_str() == Some(name))
        .unwrap()
        .as_table()
        .unwrap()
        .clone()
}

#[test]
fn retained_corpus_matches_independent_success_and_failure_expectations() {
    for (name, bytes) in FIXTURES {
        let expected = expectation(name);
        let actual_hash = format!("{:x}", Sha256::digest(bytes));
        assert_eq!(
            expected["sha256"].as_str(),
            Some(actual_hash.as_str()),
            "{name}"
        );

        match (
            expected["outcome"].as_str().unwrap(),
            decode(bytes, bytes.len(), name),
        ) {
            ("success", Ok(shell)) => {
                shell
                    .validate()
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                let encoded = shell.encode(bytes.len()).unwrap();
                assert_eq!(encoded, *bytes, "{name} was repaired or normalized");
                let roundtrip_hash = format!("{:x}", Sha256::digest(&encoded));
                assert_eq!(
                    expected["expected_roundtrip_sha256"].as_str(),
                    Some(roundtrip_hash.as_str()),
                    "{name}"
                );
            }
            ("failure", Err(error)) => {
                assert_located(&error, bytes.len());
                assert_eq!(
                    expected["failure_offset"].as_integer(),
                    Some(error.offset as i64),
                    "{name}: {error:?}"
                );
                assert_eq!(
                    expected["failure_path"].as_str(),
                    Some(error.path.as_str()),
                    "{name}: {error:?}"
                );
            }
            (outcome, result) => panic!("{name}: expected {outcome}, got {result:?}"),
        }
    }
}

#[test]
fn every_admitted_major_has_a_byte_identical_roundtrip() {
    let template = include_bytes!("../fixtures/positive.class");
    for major in MIN_MAJOR..=MAX_MAJOR {
        let mut bytes = template.to_vec();
        bytes[6..8].copy_from_slice(&major.to_be_bytes());
        let label = format!("major-{major}.class");
        let shell = decode(&bytes, bytes.len(), &label).unwrap();
        assert_eq!(shell.major_version, major);
        assert_eq!(shell.encode(bytes.len()).unwrap(), bytes, "major {major}");
    }
}

fn fuzz_inputs() -> Vec<Vec<u8>> {
    let mut corpus = FIXTURES
        .iter()
        .map(|(_, bytes)| bytes.to_vec())
        .collect::<Vec<_>>();
    let seeds = [
        include_bytes!("../fixtures/positive.class").as_slice(),
        include_bytes!("../fixtures/module-info.class").as_slice(),
        include_bytes!("../fixtures/record.class").as_slice(),
        include_bytes!("../fixtures/unknown-attribute.class").as_slice(),
    ];
    let mut state = 0x9e37_79b9_u32;
    for case in 0..512usize {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let seed = seeds[case % seeds.len()];
        let mut bytes = seed.to_vec();
        match case % 4 {
            0 => bytes.truncate((state as usize) % (bytes.len() + 1)),
            1 => {
                let at = (state as usize) % bytes.len();
                bytes[at] ^= (state >> 8) as u8 | 1;
            }
            2 => {
                let at = (state as usize) % (bytes.len() + 1);
                bytes.insert(at, state as u8);
            }
            _ => {
                let start = (state as usize) % bytes.len();
                let width = ((state >> 16) as usize % 8).min(bytes.len() - start);
                bytes.drain(start..start + width);
            }
        }
        corpus.push(bytes);
    }
    corpus
}

#[test]
fn bounded_decode_encode_and_edit_corpus_never_panics_or_repairs_input() {
    for (case, bytes) in fuzz_inputs().into_iter().enumerate() {
        let label = format!("fuzz-{case}.class");
        let result = catch_unwind(AssertUnwindSafe(|| {
            // Exercise strict allocation limits independently of structural work limits.
            if let Err(error) = decode(&bytes, bytes.len() / 2, &label) {
                assert_located(&error, bytes.len());
            }

            match decode(&bytes, bytes.len(), &label) {
                Err(error) => assert_located(&error, bytes.len()),
                Ok(mut shell) => {
                    match shell.encode(bytes.len()) {
                        Ok(encoded) => {
                            assert_eq!(encoded, bytes, "case {case} was repaired or normalized")
                        }
                        Err(error) => assert_located(&error, bytes.len()),
                    }

                    // Encoding must honor a smaller bound without partial or lossy success.
                    if !bytes.is_empty() {
                        assert!(shell.encode(bytes.len() - 1).is_err());
                    }

                    // Exercise the checked edit path when a Code body is present. Arbitrary
                    // bytecode is retained data here; structural exception-table bounds remain
                    // checked by replace_method_code.
                    let edit = shell.validate().ok().and_then(|_| {
                        shell.methods.iter().enumerate().find_map(|(method, row)| {
                            row.attributes.iter().find_map(|attribute| {
                                CodeAttribute::decode(&mut ByteReader::new(
                                    &attribute.bytes,
                                    attribute.bytes.len(),
                                ))
                                .ok()
                                .map(|code| (method, code.code))
                            })
                        })
                    });
                    if let Some((method, mut code)) = edit {
                        code.push((case & 0xff) as u8);
                        match shell.replace_method_code(method, code, bytes.len() * 2) {
                            Ok(_) => {
                                let edited = shell.encode(bytes.len() * 2).unwrap();
                                match decode(&edited, edited.len(), &label) {
                                    Ok(reparsed) => {
                                        assert_eq!(reparsed.encode(edited.len()).unwrap(), edited)
                                    }
                                    Err(error) => assert_located(&error, edited.len()),
                                }
                            }
                            Err(error) => assert_located(&error, bytes.len()),
                        }
                    }
                }
            }
        }));
        assert!(result.is_ok(), "case {case} panicked");
    }
}
