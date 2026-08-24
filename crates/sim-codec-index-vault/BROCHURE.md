# Complete Index vaults, without a second renderer

Turn one checked SIM Index projection into deterministic portable, Obsidian,
Seqlog, or Logseq Markdown bundles. Every canonical row is retained, links and
metadata use explicit v2 dialects, and semantic identity stays independent of
presentation bytes. The library is pure: callers decide where bytes go.

It reads bounded historical v1 bundles for checked migration and rejects valid
Markdown whose semantic claims were corrupted. Decode never imports notes or
writes a vault. Compatibility covers CommonMark, Obsidian Markdown, SeqLog
Markdown files, and Logseq file graphs -- not Logseq DB graphs.
