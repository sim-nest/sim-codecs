# sim-codec

In one line: It is the shared workshop that lets every SIM format read text or bytes in and write them back out.

## What it gives you

This is the common ground that all the other formats stand on. It sets the rules for turning written or stored data into a checked value, and for turning a value back into text or bytes. It also decides where a piece of output is going -- something to run, something to quote, plain data, or a pattern to match against -- so the same value can be written the right way for each setting. Because every other format in this family shares these rules, they all behave alike, guard against oversized or hostile input the same way, and register themselves the same way. You get one dependable foundation instead of a dozen slightly different ones.

## Why you will be glad

- Every format in the set reads and writes in a consistent, predictable manner.
- Oversized or malformed input is caught by shared limits, so nothing runs away.
- New formats plug in against one clear contract rather than starting from scratch.

## Where it fits

This is the base of the SIM codec family. The infix, s-expression, JSON, binary, and domain formats are all built on top of it, so it defines how reading and writing works across the whole runtime.
