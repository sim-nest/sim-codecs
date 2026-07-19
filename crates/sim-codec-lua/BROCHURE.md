# sim-codec-lua

In one line: It gives SIM a bounded Lua chunk reader and writer that keeps source tied to the shared expression graph.

## What it gives you

This crate recognizes Lua chunks, preserves the spelling of source-level values, and groups operators with Lua precedence. It covers the statement shapes builders expect in real scripts: local attributes, assignment, conditionals, loops, function declarations, returns, labels, and gotos, while keeping comments and source spans available to located and tree lanes.

## Why you will be glad

- Lua source can enter SIM through a parser built around resource limits.
- Operators, tables, calls, and blocks arrive as plain expression forms that other runtime layers can inspect.
- Located and tree lanes keep source identity available for diagnostics and faithful replay.

## Where it fits

This is a codec-family crate below the Lua language surface. It sits next to the shared Pratt parser and the core codec traits, producing `lua/*` expression forms that the Lua runtime layer can lower and execute.
