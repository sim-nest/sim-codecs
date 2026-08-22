use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sim_codec_classfile::{OPCODES, Opcode};

static NEXT_MOUNT: AtomicU64 = AtomicU64::new(1);

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_generator(root: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new("python3")
        .arg(root.join("generate_opcodes.py"))
        .args(arguments)
        .output()
        .expect("run opcode generator")
}

fn scratch_copy() -> PathBuf {
    let nonce = NEXT_MOUNT.fetch_add(1, Ordering::Relaxed);
    let destination = std::env::temp_dir().join(format!(
        "sim-codec-classfile-opcodes-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(destination.join("src")).expect("create scratch source directory");
    let source = crate_root();
    for relative in [
        "generate_opcodes.py",
        "opcode-manifest.tsv",
        "OPCODES.md",
        "src/opcode_generated.rs",
    ] {
        fs::copy(source.join(relative), destination.join(relative)).expect("copy generator input");
    }
    destination
}

#[test]
fn generated_opcode_artifacts_are_current_and_unique() {
    let output = run_generator(&crate_root(), &["--check", "--scan"]);
    assert!(
        output.status.success(),
        "opcode fixpoint failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn every_byte_has_the_matching_generated_identity() {
    assert_eq!(OPCODES.len(), 256);
    for byte in u8::MIN..=u8::MAX {
        let opcode = Opcode::from_byte(byte);
        assert_eq!(opcode as u8, byte);
        assert_eq!(opcode.metadata(), &OPCODES[usize::from(byte)]);
    }
}

#[test]
fn manifest_edits_without_regeneration_fail_the_fixpoint() {
    let scratch = scratch_copy();
    let manifest = scratch.join("opcode-manifest.tsv");
    let mut text = fs::read_to_string(&manifest).expect("read scratch manifest");
    let first_row = text
        .lines()
        .find(|line| line.starts_with("0x00\t"))
        .expect("manifest has first opcode")
        .to_owned();
    text.push_str(&first_row);
    text.push('\n');
    fs::write(manifest, text).expect("mutate scratch manifest");

    let output = run_generator(&scratch, &["--check"]);
    fs::remove_dir_all(&scratch).expect("remove scratch directory");
    assert!(
        !output.status.success(),
        "stale manifest unexpectedly passed"
    );
}

#[test]
fn a_parallel_source_inventory_is_rejected_as_a_source_fact() {
    let scratch = scratch_copy();
    let bytes = (0_u8..8)
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        scratch.join("src/parallel.rs"),
        format!("const INSTRUCTION_OPCODE_TABLE: [u8; 8] = [{bytes}];\n"),
    )
    .expect("write parallel inventory");

    let output = run_generator(&scratch, &["--check", "--scan"]);
    fs::remove_dir_all(&scratch).expect("remove scratch directory");
    assert!(
        !output.status.success(),
        "parallel opcode inventory unexpectedly passed"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("parallel opcode-like inventory"),
        "failure did not identify the parallel inventory: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
