# sim-codec-compare

A developer harness that measures `sim-codec-bitwise` against `sim-codec-binary`
on real data -- size and speed -- so the BITWISE family's density claims rest on
numbers, not faith. It owns a categorized corpus, a size measurer, a
dependency-free timing harness, and a `report` binary. `publish = false`.

Run it:

```
cargo run --release -p sim-codec-compare --bin report
```

## Findings: when is bitwise actually worth it?

Measured 2026-07-01 over the built-in corpus (size ratio = bitwise/binary, so
`< 1.0` means bitwise is smaller; slowdown = bitwise time / binary time, so
`> 1.0` means bitwise is slower).

| category   | size ratio | dense ratio | enc slowdown | dec slowdown |
|------------|-----------:|------------:|-------------:|-------------:|
| TinyScalar |      0.47  |       0.53  |       1.04x  |       1.33x  |
| SmallInts  |      0.50  |       0.50  |       1.49x  |       1.39x  |
| BigInts    |      0.48  |     **0.11**|       6.18x  |       4.18x  |
| Floats     |      0.96  |     **0.14**|       2.04x  |       1.60x  |
| Symbols    |      0.76  |       0.80  |       1.14x  |       1.08x  |
| Strings    |    **1.00**|       1.00  |     **9.15x**|       3.19x  |
| DeepNested |      0.58  |       0.66  |       1.12x  |       1.02x  |
| WideMap    |      0.53  |       0.61  |       1.05x  |       1.08x  |
| Realistic  |      0.61  |       0.61  |       1.13x  |       1.04x  |
| Repetitive |      0.56  |     **0.07**|       1.21x  |       1.06x  |

**The answer is: yes, but narrowly -- and never for strings.**

- **Structured / integer / realistic data (the common case): bitwise wins big.**
  Realistic runtime records are ~40% smaller (0.61), integer-dense vectors ~half
  (0.48-0.50), maps/nesting ~0.53-0.58 -- all at a modest ~1.0-1.5x CPU cost.
  Signed minimal-magnitude integers and the magic-free frame are the wins; they
  are real.
- **Dense mode is transformative on repetition, and only there.** Repeated
  subtrees collapse to 0.07 of binary and repeated big ints/floats to ~0.11-0.14.
  But the `Ref` table is pure overhead when there is nothing to share, so dense is
  *larger* than plain on non-repetitive data -- a targeted tool, not a default.
- **Strings are the honest null result.** Raw UTF-8 does not bit-pack: bitwise is
  a size wash (1.00) AND up to ~9x slower to encode. For string-blob-heavy
  payloads bitwise costs a lot of CPU for nothing.
- **Bitwise is never *larger* than binary** anywhere in the corpus -- the
  "smallest canonical serialization" claim holds; it just narrows to a tie on
  strings/floats.

### Recommendation (default codec choice)

- Use **bitwise** (and its `canonical_bytes`) as the default for **canonical
  storage and content-addressing** and for **structured / integer-dense runtime
  data**, where a ~40-50% size win at ~1.2x CPU is an excellent trade and the
  encode usually happens once. Use **`encode_dense`** specifically for
  content-addressed stores of **repetitive** data.
- Use **binary** on the **hot path** when encode latency dominates, and for
  **string-blob-heavy** payloads, where bitwise buys no size and costs multiples
  of the CPU.

Bitwise is not a research artifact and it is not a universal default -- it is a
sharp tool with a real, measured niche. The corpus tests in
`src/findings_tests.rs` lock these conclusions so they cannot silently regress.
