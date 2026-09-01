// conformance: the MCP schema oracle checks the delivered compatibility contract.

#[path = "../build_support/schema_oracle.rs"]
mod validator;

use std::{
    fs,
    path::{Path, PathBuf},
};

struct TestDir(PathBuf);

impl TestDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn fixture_copy() -> TestDir {
    let temp = TestDir(std::env::temp_dir().join(format!(
        "sim-codec-mcp-oracle-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    )));
    if temp.path().exists() {
        fs::remove_dir_all(temp.path()).unwrap();
    }
    fs::create_dir(temp.path()).unwrap();
    copy_tree(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .as_path(),
        &temp.path().join("fixtures"),
    );
    temp
}

#[test]
fn pinned_oracle_is_closed_and_generates_offline() {
    let temp = fixture_copy();
    let output = temp.path().join("out");
    fs::create_dir(&output).unwrap();
    validator::validate_and_generate(temp.path(), &output).unwrap();
    assert!(
        fs::read_to_string(output.join("mcp_vocabulary.rs"))
            .unwrap()
            .contains("server/discover")
    );
}

#[test]
fn removing_a_ledger_row_names_the_exact_uncovered_source_path() {
    let temp = fixture_copy();
    let path = temp.path().join("fixtures/mcp/2026-07-28/coverage.json");
    let mut ledger: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let removed = ledger["entries"].as_array_mut().unwrap().remove(0);
    fs::write(&path, serde_json::to_vec(&ledger).unwrap()).unwrap();
    let error = validator::validate_and_generate(temp.path(), temp.path()).unwrap_err();
    assert_eq!(
        error,
        format!(
            "uncovered source path: {}",
            removed["sourcePath"].as_str().unwrap()
        )
    );
}

#[test]
fn removing_a_schema_definition_names_the_orphaned_ledger_path() {
    let temp = fixture_copy();
    let path = temp.path().join("fixtures/mcp/2026-07-28/schema.json");
    let mut schema: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let removed = schema["definitions"].as_array_mut().unwrap().remove(0);
    fs::write(&path, serde_json::to_vec(&schema).unwrap()).unwrap();
    let error = validator::validate_and_generate(temp.path(), temp.path()).unwrap_err();
    assert!(
        error.contains(removed["sourcePath"].as_str().unwrap()),
        "{error}"
    );
}

#[test]
fn an_unclassified_open_map_is_impossible() {
    let temp = fixture_copy();
    let path = temp.path().join("fixtures/mcp/2026-07-28/schema.json");
    let mut schema: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let open = schema["definitions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["kind"] == "extension")
        .unwrap();
    open.as_object_mut().unwrap().remove("openExtensionReason");
    fs::write(&path, serde_json::to_vec(&schema).unwrap()).unwrap();
    let error = validator::validate_and_generate(temp.path(), temp.path()).unwrap_err();
    assert!(error.starts_with("unclassified open map:"), "{error}");
}

#[test]
fn only_exact_delivered_profiles_exist() {
    assert_eq!(sim_codec_mcp::protocol_profiles()[0].revision, "2025-03-26");
    assert_eq!(sim_codec_mcp::protocol_profiles()[1].revision, "2026-07-28");
    assert_eq!(
        sim_codec_mcp::modern_schema().definitions.len(),
        sim_codec_mcp::coverage_ledger().entries.len()
    );
}
