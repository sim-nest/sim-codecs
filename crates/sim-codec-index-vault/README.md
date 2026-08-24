# sim-codec-index-vault

`sim-codec-index-vault` is the pure domain codec from a complete
`VaultProjection` to a deterministic v2 Markdown bundle. It owns profiles,
paths, semantic projection identity, and bundle identity. It never opens a
path and never changes canonical Index records.

All Markdown bytes come from `sim-codec-doc`. The codec preserves the exact
canonical row as a typed, family-labelled semantic block and requires a closed
projection claim certificate before encoding.

```rust
let projection = VaultProjection::from_complete(&index, VaultGranularity::Compact)?;
let bundle = VaultEncoder::new(resolve_profile("obsidian")?).encode(&projection)?;
let checked = VaultDecoder::new(bundle.profile).decode(&bundle)?;
assert_eq!(checked.projection, projection);
# Ok::<(), sim_codec_index_vault::VaultCodecError>(())
```

Decode is pure semantic verification: it never imports notes, opens paths, or
mutates a vault. The checked conformance specimen covers all-profile semantic
round trips, syntactically valid semantic corruption, exact row coverage, and
bounded v1 migration.

## Bounds

Encoding rejects more than 100,000 rows, 50,000 notes, a 1 MiB note, a 128 MiB
bundle, or an invalid/colliding normalized path.

## v2 compatibility

The four write profiles are `portable-markdown-v2`, `obsidian-markdown-v2`,
`seqlog-markdown-v2`, and `logseq-file-v2`. Friendly aliases select those ids.
They are semantic v2 contracts, not byte-compatible names for the historical
v1 renderer.

## External compatibility contracts

These dates freeze the minimal fixtures exercised by the conformance specimen;
they are evidence boundaries, not claims of full application emulation.

| Profile | Contract observed | Minimal fixture |
| --- | --- | --- |
| CommonMark 0.31.2 | 2026-08-24 | JSON front matter, headings, lists, and ordinary Markdown links |
| Obsidian Markdown | 2026-08-24 | Properties-compatible front matter and internal wikilinks |
| SeqLog Markdown files | 2026-08-24 | Plain CommonMark files with deterministic metadata |
| Logseq file graph | 2026-08-24 | Double-colon properties, wikilinks, and indented lists |

Logseq DB-graph compatibility is explicitly out of scope.
