# sim-codec-binary

In one line: It packs any value into a small, tagged stream of bytes and reads it straight back.

## What it gives you

This is the compact byte form for SIM values. Instead of readable text, it writes a tight, tagged frame: a short header, a few shared tables that record repeated names once, and a body that lays out the value efficiently. It can also carry the source position of each part, so values that remember where they came from survive the trip. Reading is guarded by strict limits, so a broken or hostile stream is refused rather than followed. This is the form to reach for when size on disk or over the wire matters, or when you want a value stored and recovered exactly. Arbitrary bytes are treated as untrusted data, never as instructions to run.

## Why you will be glad

- Values take up far less room than their readable text form.
- Repeated names are stored once, so common shapes compress naturally.
- Malformed or hostile input is refused up front rather than acted on.

## Where it fits

This is the binary member of the SIM codec family. Where the infix and s-expression surfaces are for people to read, this one is for machines to store and move values compactly and recover them intact.
