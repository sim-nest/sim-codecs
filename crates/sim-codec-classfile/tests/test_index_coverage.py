#!/usr/bin/env python3
"""Focused fixture proof for the classfile Index coverage policy."""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
INPUTS = (
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
)


class CoveragePolicyFixtures(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="sim-classfile-index-")
        self.root = pathlib.Path(self.temporary.name)
        (self.root / "src").mkdir()
        for relative in INPUTS:
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, target)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_policy(self) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["python3", str(self.root / "generate_index_coverage.py"), "--check", "--scan", "--workspace-root", str(self.root)],
            text=True,
            capture_output=True,
            check=False,
        )

    def test_clean_projection_has_zero_difference(self) -> None:
        result = self.run_policy()
        self.assertEqual(result.returncode, 0, result.stderr)
        projection = (self.root / "CLASSFILE_COVERAGE.md").read_text()
        self.assertIn("256 opcodes", projection)
        self.assertIn("coverage difference: 0", projection)

    def test_duplicate_inventory_fails(self) -> None:
        (self.root / "src/duplicate.rs").write_text("const OPCODE_TABLE: &[u8] = &[0, 1, 2];\n")
        self.assertIn("duplicate classfile inventory", self.run_policy().stderr)

    def test_runtime_byte_parser_fails(self) -> None:
        source = self.root / "crates/sim-lib-jvm-runtime/src"
        source.mkdir(parents=True)
        (source.parent / "Cargo.toml").write_text('[package]\nname = "sim-lib-jvm-runtime"\n')
        (source / "parser.rs").write_text("fn classfile(r: &mut ByteReader) { r.read_u2(); }\n")
        self.assertIn("parses classfile bytes outside", self.run_policy().stderr)

    def test_lossy_modified_utf8_fails(self) -> None:
        (self.root / "src/lossy.rs").write_text("fn modified_utf8(b: &[u8]) { String::from_utf8_lossy(b); }\n")
        self.assertIn("lossy modified UTF-8 crossing", self.run_policy().stderr)

    def test_hand_edited_generated_file_fails(self) -> None:
        (self.root / "CLASSFILE_COVERAGE.md").write_text("hand edited\n")
        self.assertIn("generated classfile coverage is stale", self.run_policy().stderr)


if __name__ == "__main__":
    unittest.main()
