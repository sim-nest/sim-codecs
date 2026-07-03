# sim-codec-json

In one line: It reads and writes any value as JSON, so SIM data flows through the world's most common interchange format.

## What it gives you

JSON is the shared language of the web and of countless tools, and this brings SIM into it fully. It can write any value the runtime holds as JSON and read it straight back with nothing lost, using a tagged form that keeps every detail exact. When you need to hand data to an outside program that expects plain, ordinary JSON, it also offers a simpler untagged view, plus a compact description of a value's shape. So you get two modes from one place: a faithful round-trip for your own data, and a friendlier surface for talking to systems that were not built for SIM. Either way, values move in and out through a format almost everything already understands.

## Why you will be glad

- Any value round-trips through JSON with nothing lost.
- Outside tools can receive plain, untagged JSON they already understand.
- A shape description lets other systems know what to expect.

## Where it fits

This is the JSON member of the SIM codec family, sitting beside the infix and s-expression surfaces. It is the natural bridge whenever SIM data has to meet the wider world of web services, files, and tools that speak JSON.
