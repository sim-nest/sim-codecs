# sim-codec-javascript

Bounded, lossless ECMAScript 2026 Script and Module source frontend. It emits a
neutral concrete tree with byte spans, trivia, lexical-goal and ASI evidence;
it does not evaluate source or construct compiler IR.

The frozen authority and evidence corpus identity live in `src/lib.rs` and
`grammar/corpus-manifest.txt`. Test262 is an oracle, not vendored code or a
blanket conformance claim.
