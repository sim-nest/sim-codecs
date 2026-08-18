# Bounded JVM classfile data

In one line: It turns JVM classfiles into bounded, lossless SIM data that can be inspected, edited, and written back without running Java code.

## What it gives you

This codec reads the complete byte-oriented structure of a Java Virtual Machine classfile into explicit SIM values. Constant-pool entries, methods, fields, attributes, stack maps, annotations, modules, records, bootstrap data, and every opcode retain the evidence needed to understand their original layout. Instruction rows carry absolute byte offsets, modified UTF-8 preserves code units that ordinary text cannot represent, and bounded readers reject malformed or oversized inputs predictably. A validated shell supports controlled edits while tracking which offsets and nested layouts must be rebuilt. Encoding restores a retained projection to classfile bytes instead of silently discarding unfamiliar data.

## Why you will be glad

- Tools can browse classfiles without loading classes or granting execution authority.
- Lossless retention keeps unknown attributes and exact text evidence available for round trips.
- Explicit budgets, offsets, and typed errors make hostile binary input manageable.
- One generated opcode inventory keeps decoding, encoding, documentation, and verification aligned.

## Where it fits

This is the format boundary below the loadable JVM runtime. It owns classfile bytes and structure, while class loading, verification, linking, execution, capabilities, and guest policy remain in runtime libraries.
