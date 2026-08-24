# sim-codec-index-vault

`sim-codec-index-vault` is the pure domain codec from a complete
`VaultProjection` to a deterministic v2 Markdown bundle. It owns profiles,
paths, semantic projection identity, and bundle identity. It never opens a
path and never changes canonical Index records.

All Markdown bytes come from `sim-codec-doc`. The codec preserves the exact
canonical row as a typed, family-labelled semantic block and requires a closed
projection claim certificate before encoding.

## Bounds

Encoding rejects more than 100,000 rows, 50,000 notes, a 1 MiB note, a 128 MiB
bundle, or an invalid/colliding normalized path.

## v2 compatibility

The four write profiles are `portable-markdown-v2`, `obsidian-markdown-v2`,
`seqlog-markdown-v2`, and `logseq-file-v2`. Friendly aliases select those ids.
They are semantic v2 contracts, not byte-compatible names for the historical
v1 renderer.
