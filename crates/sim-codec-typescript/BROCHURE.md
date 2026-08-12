# TypeScript syntax without a compiler dependency

In one line: It layers bounded TypeScript 7.0.2 and TSX syntax over SIM's JavaScript frontend while keeping compiler-dependent decisions explicit.

## What it gives you

The crate reuses the public JavaScript token and node model for ECMAScript identity, then adds TypeScript declarations, annotations, modifiers, type nodes, and JSX metadata. Exact source, trivia, byte locations, and extension context survive in the returned tree. Direct lowering erases only syntax that needs no compiler judgment and builds `javascript/*` expressions through `JavascriptBuilder`, retaining annotation references and derivation origins. Checker-dependent assertions, code-producing constructs, decorators, and JSX transforms remain located `EvaluationGap` values instead of receiving approximate semantics.

## Why you will be glad

- JavaScript grammar behavior has one owner rather than a TypeScript fork.
- Tooling can inspect faithful annotations even when runtime execution is intentionally unavailable.
- Unsupported compiler lanes fail closed at precise source locations.
- Plain, located, and tree codec lanes share the same bounded admission rules.

## Where it fits

This is the syntax and codec layer below `sim-lib-lang-typescript`. It provides no checker, project graph, compiler, emitter, or second runtime. Erasable programs ultimately use the existing JavaScript evaluator and capability model.
