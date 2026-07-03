# sim-codec-binary-base64

In one line: It carries the compact binary form as plain text, so it can travel anywhere only text is allowed.

## What it gives you

Some channels only accept plain text -- an email body, a web field, a log line, a config value. This lets the compact binary form ride along on those channels. It takes the same tight byte frame the binary format produces and wraps it as an ordinary run of readable characters, and it unwraps that text back into the exact bytes on the other side. Nothing about the value changes; you simply get a text-safe envelope around it. That means you can drop a full value into a place that would otherwise reject raw bytes, then recover it later without loss. Text that is not a valid envelope is refused rather than guessed at.

## Why you will be glad

- Full values fit into text-only fields, messages, and logs without special handling.
- The wrapped value is recovered byte for byte, with nothing lost.
- Invalid text is turned away instead of being misread.

## Where it fits

This is the text-transport member of the SIM codec family. It is a thin layer over the binary format, letting the same compact frames move through channels that accept only plain characters.
