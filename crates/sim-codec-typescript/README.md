# sim-codec-typescript

Bounded, lossless TypeScript 7.0.2 and TSX syntax layered downstream of
`sim-codec-javascript`. ECMAScript productions retain the JavaScript frontend's
public node and token types; this crate adds TypeScript declarations,
annotations, type nodes, modifiers, and JSX metadata without checker, compiler,
lowering, project, or runtime behavior.

`parse_module` selects TypeScript notation and `parse_tsx` additionally admits
JSX. Both retain exact source, trivia, byte locations, and parser context.
