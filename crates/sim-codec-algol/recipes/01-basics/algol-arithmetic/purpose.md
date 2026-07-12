# Compute an arithmetic expression

The Algol codec is a Pratt-parsed textual expression surface. Parsed in eval
position, `1 + 2 * 3` lowers (with `*` binding tighter than `+`) to the call
`(+ 1 (* 2 3))`, which the runtime evaluates to `7`. The conformance surface
is not just a grammar -- it computes.
