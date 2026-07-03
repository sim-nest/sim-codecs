# sim-codec-lisp

In one line: It reads and writes values in parenthesized s-expression text, the plain nested form where structure is spelled out with brackets.

## What it gives you

This is the bracket-and-list surface for SIM. It reads text written as nested parenthesized forms and turns it into a checked value, and it writes any value back out in that same clear, nested style. When it writes, it pays attention to the setting -- whether the value is meant to be run, quoted, kept as plain data, or used as a pattern -- and shapes the text to suit. Because it covers the full range of values rather than one narrow kind, anything the runtime can hold travels through it and comes back with the same meaning. The nested form makes the shape of a value visible right on the page, which is why it doubles as a faithful, readable way to store and inspect data.

## Why you will be glad

- The nested brackets make a value's structure plain to see.
- Any value round-trips, so meaning is preserved between reading and writing.
- The written form suits its setting, whether for running, quoting, or plain data.

## Where it fits

This is one of the general-purpose surfaces in the SIM codec family, alongside the infix and JSON forms. It offers the classic parenthesized style for people who want a value's shape laid bare.
