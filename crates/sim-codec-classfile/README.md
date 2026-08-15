# sim-codec-classfile

This crate will provide the bounded, lossless JVM classfile codec. Its parser is
deliberately absent in the scope-freeze phase. `scope.toml` is the machine-readable
format and reuse contract; `fixtures/expectations.toml` freezes independently
authored outcomes against retained classfile bytes before decoding exists.

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

