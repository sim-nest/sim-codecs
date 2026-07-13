# sim-codec-doc

In one line: It reads and writes Markdown, Typst, AsciiDoc, and LaTeX as one structured document value.

## What it gives you

This treats a document as more than a flat wall of text. It reads Markdown, Typst, AsciiDoc, and LaTeX into one organized value with real parts -- blocks, sections, math, tables, source fragments, and chunks -- that the runtime can hold and hand around. It can write that structure back out through the supported markup formats. Along the way it can split a document into chunks while keeping track of where each piece came from, so a passage always remembers its place in the whole. That makes it easy to pull a document apart for review, search, or processing and still trust the origin of every fragment. It names any loss or preserved raw material instead of pretending everything translated cleanly.

## Why you will be glad

- Common technical markup formats become organized, workable structure.
- Documents split into chunks that remember exactly where they came from.
- The written form comes back out through a supported markup format again.

## Where it fits

This is a focused, domain member of the SIM codec family. Rather than handling every kind of value, it specializes in documents, giving the runtime a dependable way to read, divide, and rewrite written material while other markup families stay explicitly outside the accepted set.
