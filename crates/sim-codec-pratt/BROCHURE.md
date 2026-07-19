# sim-codec-pratt

In one line: It gives SIM codecs a shared way to group infix tokens into expression trees.

## What it gives you

This crate keeps precedence parsing in one place for text surfaces that use infix operators. A codec supplies its own lexer and operator table, then receives a located expression tree with the same grouping rules, source spans, and resource limits each time. That keeps language-specific crates focused on their syntax while the common parser handles the careful parts of binding, calls, prefixes, postfixes, and nested input.

## Why you will be glad

- Infix codecs share one tested grouping engine instead of copying parser loops.
- Source spans and trivia stay attached to the tree for readable round-trips.
- Decode limits apply consistently across token streams.

## Where it fits

This is a substrate crate in the SIM codec family. Concrete text codecs use it below their public read and write surfaces, alongside the core codec traits and the kernel expression model.
