# sim-codec-bridge

In one line: It gives SIM, people, and model seats one checked packet format for requests, replies, reviews, and receipts.

## What it gives you

BRIDGE makes an exchange inspectable from the first byte. A packet names who speaks, who receives it, what move it makes, what context it cites, and what typed parts it carries. This crate reads and writes the strict line form for that packet and checks the book of allowed parts and moves before the packet is trusted. The result is a narrow entry point where collaboration messages have stable identity, clear structure, and no hidden side channel.

## Why you will be glad

- A packet can be replayed and compared by content, not by guesswork.
- Dialogue moves are checked against the same book on every side.
- Unknown or malformed packet text is refused before it enters the runtime.

## Where it fits

This is the BRIDGE member of the SIM codec family. It supplies the shared packet envelope used by model-facing, human-facing, and runtime-facing bridge libraries, while leaving transport, execution, and repair policy to those libraries.
