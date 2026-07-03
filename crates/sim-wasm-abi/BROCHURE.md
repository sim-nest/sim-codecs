# sim-wasm-abi

In one line: It is the shared handshake that lets SIM pass values to and from sandboxed WebAssembly modules.

## What it gives you

WebAssembly lets code from elsewhere run safely inside a sandbox, and this is the agreed way for SIM and such a module to talk. It defines the exact byte frames that carry values, descriptions, and lists of what a module offers, so both sides read and write them the same way. With that handshake in place, a guest module can be brought in and its offerings surfaced to the runtime as if they were part of it, ready to be called. This handles only the crossing itself -- the framing and the passing of values back and forth -- and leaves what the guest actually does inside its own walls. The result is a clean, well-defined border between the host and any code loaded into the sandbox.

## Why you will be glad

- Sandboxed modules exchange values with SIM through one clear, shared framing.
- A guest's offerings appear to the runtime as ready-to-call parts.
- The border between host and guest stays clean, so each side keeps its own concerns.

## Where it fits

This is the WebAssembly crossing point for the SIM runtime. It gives the host and a sandboxed guest a common way to hand values across the boundary, so outside code can be loaded and used without blurring the line between them.
