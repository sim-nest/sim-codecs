# sim-codec-lua

In one line: It lets SIM read Lua chunks into shared expression forms with source identity kept nearby.

## What it gives you

This crate recognizes Lua chunks, preserves the spelling of source-level values, and groups operators with Lua precedence. It covers the statement shapes builders expect in real scripts: local attributes, assignment, conditionals, loops, function declarations, returns, labels, and gotos. Plain, located, and tree lanes keep comments and source spans available for diagnostics, conformance checks, and faithful replay.

## Why you will be glad

- Lua source can enter SIM through a parser built around resource limits.
- Operators, tables, calls, and blocks arrive as expression forms that runtime layers can inspect.
- Located and tree lanes keep source identity available when tools need exact context.

## Where it fits

This is a codec-family crate below the Lua language surface. It sits next to the shared Pratt parser and the core codec traits, producing Lua expression forms that the Lua runtime layer can lower and execute.
