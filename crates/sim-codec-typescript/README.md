# sim-codec-typescript

Bounded, lossless TypeScript 7.0.2 and TSX syntax layered downstream of
`sim-codec-javascript`. ECMAScript productions retain the JavaScript frontend's
public node and token types; this crate adds TypeScript declarations,
annotations, type nodes, modifiers, and JSX metadata without checker, compiler,
project, or TypeScript runtime behavior.

`parse_module` selects TypeScript notation and `parse_tsx` additionally admits
JSX. Both retain exact source, trivia, byte locations, and parser context.

`lower_typescript` erases only syntax requiring no compiler decision and builds
the resulting `javascript/*` graph directly through `JavascriptBuilder`. It
retains annotation references and complete derivation origins. Checker-dependent
assertions and `satisfies`, code-producing enums and namespaces, parameter
properties, decorators, and JSX/TSX transforms remain lossless, located
`EvaluationGap` values. The plain, located, and tree decode lanes share this
firewall; encoding uses canonical JavaScript-compatible text or the established
tagged expression fallback, without emit-and-reparse.
