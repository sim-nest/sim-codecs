// conformance: classfile coverage policy rejects duplicate, lossy, and hand-edited inventories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_MOUNT: AtomicU64 = AtomicU64::new(1);

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(root: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new("python3")
        .arg(root.join("generate_index_coverage.py"))
        .args(arguments)
        .output()
        .expect("run classfile Index coverage generator")
}

fn scratch_copy() -> PathBuf {
    let nonce = NEXT_MOUNT.fetch_add(1, Ordering::Relaxed);
    let destination = std::env::temp_dir().join(format!(
        "sim-codec-classfile-index-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(destination.join("src")).expect("create scratch source directory");
    let source = crate_root();
    for relative in [
        "generate_index_coverage.py",
        "index-coverage.toml",
        "opcode-manifest.tsv",
        "CLASSFILE_COVERAGE.md",
        "src/constant.rs",
        "src/constant/model.rs",
        "src/constant/codec.rs",
        "src/attribute.rs",
        "src/attribute/basic.rs",
        "src/attribute/annotations.rs",
        "src/attribute/code.rs",
        "src/attribute/class.rs",
        "src/opcode_generated.rs",
        "OPCODES.md",
    ] {
        let target = destination.join(relative);
        fs::create_dir_all(target.parent().unwrap()).expect("create coverage input parent");
        fs::copy(source.join(relative), target).expect("copy coverage input");
    }
    destination
}

fn failure(root: &Path) -> String {
    let output = run(
        root,
        &[
            "--check",
            "--scan",
            "--workspace-root",
            root.to_str().unwrap(),
        ],
    );
    assert!(
        !output.status.success(),
        "violating fixture unexpectedly passed"
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn published_coverage_is_current_and_exact() {
    let output = run(&crate_root(), &["--check", "--scan"]);
    assert!(
        output.status.success(),
        "coverage check failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let projection = fs::read_to_string(crate_root().join("CLASSFILE_COVERAGE.md")).unwrap();
    assert!(projection.contains("256 opcodes"));
    assert!(projection.contains("coverage difference: 0"));
}

#[test]
fn duplicate_inventory_fixture_is_rejected() {
    let scratch = scratch_copy();
    fs::write(
        scratch.join("src/duplicate.rs"),
        "const OPCODE_TABLE: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];\n",
    )
    .unwrap();
    assert!(failure(&scratch).contains("duplicate classfile inventory"));
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn runtime_byte_parser_fixture_is_rejected() {
    let scratch = scratch_copy();
    let member = scratch.join("crates/sim-lib-jvm-runtime/src");
    fs::create_dir_all(&member).unwrap();
    fs::write(
        member.parent().unwrap().join("Cargo.toml"),
        "[package]\nname = \"sim-lib-jvm-runtime\"\n",
    )
    .unwrap();
    fs::write(
        member.join("parser.rs"),
        "fn parse_classfile(r: &mut ByteReader) { let _ = r.read_u2(); }\n",
    )
    .unwrap();
    assert!(failure(&scratch).contains("parses classfile bytes outside sim-codec-classfile"));
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn lossy_modified_utf8_fixture_is_rejected() {
    let scratch = scratch_copy();
    fs::write(
        scratch.join("src/lossy.rs"),
        "fn modified_utf8(bytes: &[u8]) { let _ = String::from_utf8_lossy(bytes); }\n",
    )
    .unwrap();
    assert!(failure(&scratch).contains("lossy modified UTF-8 crossing"));
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn hand_edited_generated_projection_fixture_is_rejected() {
    let scratch = scratch_copy();
    fs::write(scratch.join("CLASSFILE_COVERAGE.md"), "hand edited\n").unwrap();
    assert!(failure(&scratch).contains("generated classfile coverage is stale"));
    fs::remove_dir_all(scratch).unwrap();
}
