# sim-codec-python

Bounded, lossless Python 3.14.6 source tokenization and concrete syntax trees.
This crate is a syntax artifact only: it performs no lowering or execution.

The frozen production and corpus identities live under `grammar/`. Parse with
`parse_module`; the returned tree preserves the exact input through
`SyntaxTree::preserve_source` and reports stable located diagnostics on failure.
