# sim-codec-javascript

In one line: parse ECMAScript 2026 source predictably while retaining every byte.

The frontend separates Script and Module goals, records parser-controlled
division/RegExp decisions and automatic semicolon boundaries, and exposes a
runtime-independent tree that TypeScript can extend without a reverse dependency.
