# sim-codec-config

In one line: It turns small SIM configuration files into ordinary runtime maps and writes those maps back as clean text.

## What it gives you

SIM libraries often need a few plain settings: enabled helpers, preferred defaults, limits, and load lists. This crate gives those settings a compact text format that decodes into the same map values the rest of the runtime already understands. A library can read its own table, while a launcher can read one shared file that groups many library tables by id.

## Why you will be glad

- One small format covers both per-library files and one shared launcher file.
- Parsed settings become normal map values, so existing map tools can inspect and merge them.
- Reports can write the effective settings back to predictable text.

## Where it fits

This is the configuration member of the SIM codec family. It sits beside Lisp and JSON: those codecs are also valid configuration inputs when they decode to maps, while this crate supplies the plain text form meant for hand-edited settings.
