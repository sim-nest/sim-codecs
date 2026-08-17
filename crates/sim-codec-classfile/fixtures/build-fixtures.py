#!/usr/bin/env python3
"""Build the retained corpus with javac, plus intentionally malformed cases."""

from pathlib import Path
import shutil
import struct
import subprocess

ROOT = Path(__file__).resolve().parent
SOURCES = ROOT / "sources"
BUILD = ROOT / ".build"

shutil.rmtree(BUILD, ignore_errors=True)
BUILD.mkdir()
subprocess.run(
    ["javac", "--release", "17", "-d", str(BUILD), *map(str, sorted(SOURCES.rglob("*.java")))],
    check=True,
)

for source, retained in {
    "Positive.class": "positive.class",
    "module-info.class": "module-info.class",
    "fixture/Point.class": "record.class",
    "fixture/Shape.class": "sealed.class",
    "fixture/Marker.class": "annotation.class",
    "fixture/Dynamic.class": "dynamic.class",
}.items():
    shutil.copyfile(BUILD / source, ROOT / retained)

(ROOT / "negative.class").write_bytes(bytes.fromhex("ca fe ba"))
(ROOT / "adversarial.class").write_bytes(bytes.fromhex("ca fe ba be 00 00 00 3d ff ff"))

# Minimal Java 17 class with one unrecognized, zero-length class attribute.
utf8 = lambda value: b"\x01" + struct.pack(">H", len(value)) + value
pool = [utf8(b"UnknownFixture"), b"\x07\x00\x01", utf8(b"java/lang/Object"), b"\x07\x00\x03", utf8(b"FuturePayload")]
unknown = bytes.fromhex("ca fe ba be 00 00 00 3d")
unknown += struct.pack(">H", len(pool) + 1) + b"".join(pool)
unknown += struct.pack(">HHHHHHH", 0x0021, 2, 4, 0, 0, 0, 1)
unknown += struct.pack(">HI", 5, 0)
(ROOT / "unknown-attribute.class").write_bytes(unknown)

shutil.rmtree(BUILD)
