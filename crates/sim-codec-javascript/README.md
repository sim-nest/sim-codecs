# sim-codec-javascript

Bounded, lossless ECMAScript 2026 Script and Module source frontend. It emits a
neutral concrete tree with byte spans, trivia, lexical-goal and ASI evidence;
it does not evaluate source or construct compiler IR.

The loadable `codec/javascript` lowers all accepted Script and Module syntax to
stable `javascript/*` `Expr` forms. Its plain, located, and recursively located
tree lanes encode lowered forms canonically and use the shared `__sim_expr__`
tagged fallback for expressions JavaScript cannot spell. `JavascriptBuilder`
is the supported downstream extension seam: wrappers can reuse node and token
lowering, add namespaced forms, and retain parent origins without copying the
parser. Parser coverage is deliberately independent of executable support.

The frozen authority and evidence corpus identity live in `src/lib.rs` and
`grammar/corpus-manifest.txt`. Test262 is an oracle, not vendored code or a
blanket conformance claim.
