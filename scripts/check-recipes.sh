#!/usr/bin/env sh
set -eu

# Workspace tests validate every embedded codec recipe and the classfile
# recipe-conformance target executes the retained binary setup end to end.
cargo test --workspace --quiet

printf 'check-recipes: OK (embedded codec recipes + retained classfile execution)\n'
