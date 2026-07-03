# sim-codec-bitwise-base64

In one line: It carries the canonical minimal bit-packed form as plain text, so it can travel anywhere only text is allowed.

## What it gives you

Some channels only accept plain text -- an email body, a web field, a log line, a config value. This lets the smallest canonical form ride along on those channels. It takes the tight bit-packed frame the bitwise format produces and wraps it as an ordinary run of readable characters, and it unwraps that text back into the exact bytes on the other side. Nothing about the value changes; you simply get a text-safe envelope around it. That means you can drop a full value into a place that would otherwise reject raw bytes, then recover it later without loss. Text that is not a valid envelope is refused rather than guessed at.

## Why you will be glad

- Full values fit into text-only fields, messages, and logs without special handling.
- The wrapped value is recovered by value, with nothing lost.
- Invalid text is turned away instead of being misread.

## Where it fits

This is the text-transport member of the SIM codec family for the bitwise format. It is a thin layer over the canonical minimal codec, letting the same tight frames move through channels that accept only plain characters. It shares one base64 implementation with the binary text wrapper rather than forking it.
