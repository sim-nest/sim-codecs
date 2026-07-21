# sim-codec-index

In one line: It gives the SIM Index one checked wire surface instead of many ad hoc readers.

## What it gives you

The SIM Index describes features, examples, surfaces, and routes across the whole constellation. This crate gives that graph a single codec that reads the canonical index form, validates every reference through the shared index model, and writes the same facts back as s-expression or JSON text.

## Why you will be glad

- Tools can share one checked index format instead of each inventing a parser.
- Bad ids, repeated keys, missing specimens, and open grammar claims fail before the index reaches readers.
- The JSON view stays a projection of the same graph, so dashboards and command-line tools agree.

## Where it fits

This is the index member of the SIM codec family. It sits between the generated index fragments and consumers such as search, route cards, docs, and doctor commands, while the checked graph records remain owned by `sim-index-core`.
