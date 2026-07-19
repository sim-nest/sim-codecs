# sim-codec-lua

In one line: It gives SIM a bounded reader foundation for Lua-flavored expressions.

## What it gives you

This crate recognizes Lua expression text, preserves the spelling of numbers and strings, and groups operators with Lua precedence. It covers the expression shapes builders naturally try first: arithmetic, logical operators, concatenation, table literals, field access, method calls, long strings, comments, and hexadecimal numeric spelling.

## Why you will be glad

- Lua source can enter SIM through a parser built for resource limits from the start.
- Operator grouping follows the Lua reference order instead of borrowing another surface.
- Tables, calls, and comments stay visible to codec and language layers.

## Where it fits

This is a codec-family crate below the Lua language surface. It sits next to the shared Pratt parser and the core codec traits, producing Lua expression trees that chunk decoding and runtime lowering consume.
