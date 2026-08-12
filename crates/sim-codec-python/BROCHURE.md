# Python source without hidden execution

In one line: It admits Python 3.14.6 source into SIM as bounded, lossless syntax without importing a Python runtime.

## What it gives you

`sim-codec-python` tokenizes and parses modules into a concrete tree that retains exact text, comments, layout, literal spelling, byte locations, f-strings, template strings, and soft-keyword decisions. Its loadable `codec/python` lowers accepted syntax to stable `python/*` expression forms while preserving origins and marking runtime support separately. Plain, located, and recursively located tree lanes share one limits contract. Encoding emits canonical Python-compatible forms when possible and uses the established tagged expression fallback when ordinary Python source cannot represent a SIM value.

## Why you will be glad

- Syntax fidelity does not depend on CPython, bytecode, compiler IR, or a host import search.
- Stable diagnostics and exact-source preservation support review, replay, formatters, and conformance tools.
- Parser acceptance can advance without pretending that every accepted construct is executable.
- Bounded admission gives hosts a clear resource policy for agent-authored source.

## Where it fits

This crate is the source frontend below `sim-lib-lang-python`. It owns syntax and codec behavior only. Evaluation, objects, imports, capabilities, and the public fidelity inventory belong to the loadable Python language profile and shared SIM runtime organs.
