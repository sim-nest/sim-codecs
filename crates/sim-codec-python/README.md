# sim-codec-python

Bounded, lossless Python 3.14.6 source tokenization, concrete syntax trees, and
the loadable general-purpose `codec/python` expression codec. The codec lowers
syntax to stable `python/*` forms, retaining source origins and marking future
runtime support independently; it performs no execution and creates no
bytecode or control-flow IR.

This is the source frontend for embedded, capability-scoped Python scripting,
not a CPython replacement. It never imports or invokes CPython and contains no
bytecode, compiler IR, optimizer, project graph, object model, VM, host access,
or private language organ. Syntax coverage is intentionally independent from
the direct-evaluation and library fidelity published by `sim-lib-lang-python`.

The frozen production and corpus identities live under `grammar/`. Parse with
`parse_module`; the returned tree preserves the exact input through
`SyntaxTree::preserve_source` and reports stable located diagnostics on failure.
Plain, located, and tree decoding share the codec limits. Canonical encoding
re-emits validated Python forms and uses the tagged expression fallback for SIM
expressions that Python source cannot otherwise spell.
