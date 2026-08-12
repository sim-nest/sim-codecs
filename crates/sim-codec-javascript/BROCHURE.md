# sim-codec-javascript

In one line: It gives SIM a bounded, lossless ECMAScript 2026 frontend whose source evidence survives every downstream decision.

## What it gives you

The crate parses both Script and Module goals into a runtime-independent concrete tree. Byte spans, comments, trivia, parser-controlled division-versus-RegExp choices, and automatic-semicolon evidence remain attached, so diagnostics and source tools do not have to reconstruct what the parser knew. The loadable `codec/javascript` lowers accepted syntax to stable `javascript/*` expressions and offers plain, located, and recursively located tree lanes. Canonical encoding uses JavaScript text where the language can carry an expression and the shared tagged fallback where it cannot.

## Why you will be glad

- Parser coverage can grow independently of executable runtime coverage.
- Resource limits and stable locations make untrusted source admission predictable.
- `JavascriptBuilder` lets TypeScript and other downstream tools extend lowering without copying the parser or reversing the dependency boundary.
- Script/Module distinctions, lexical goals, and ASI decisions stay explicit instead of becoming hidden parser folklore.

## Where it fits

This is the source and codec layer below `sim-lib-lang-javascript`. It recognizes and preserves ECMAScript; it does not evaluate code, construct compiler IR, or provide host services. Runtime policy, capabilities, objects, jobs, and modules remain in loadable runtime crates.
