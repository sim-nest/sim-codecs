# sim-codec-classfile

The structured attributes in this crate are lossless classfile data only. Debug,
nest, record, sealed-class, package, and module metadata do not perform runtime
resolution or lookup and carry no runtime meaning here.

This crate provides the bounded, lossless `codec/classfile` runtime. Decoding is
inert and JVM-free: retained bytes browse as bounded constants, attributes, and
instruction rows, and every instruction row carries an absolute byte offset back
into the retained classfile. Encoding a retained projection restores those bytes.
`scope.toml` remains the machine-readable format and reuse contract.

## Reuse ledger

- Reuse `sim-codec`'s `Input`, `Output`, `DecodeBudget`, and loadable codec
  protocol rather than introducing a parallel runtime surface.
- Compose `sim-codec-binary`'s bounded reader/writer approach for byte lanes and
  allocation ceilings; classfile-specific modified UTF-8 remains new work.
- Follow `sim-codec-json`'s decoder/encoder object and `Lib` registration pattern.
- Use `sim-text`'s public code-unit string representation for modified UTF-8
  values that cannot be represented as Unicode scalar strings.
- No existing crate owns JVM classfile structure, constant-pool semantics, or
  classfile attribute preservation, so this crate is the new format owner.
