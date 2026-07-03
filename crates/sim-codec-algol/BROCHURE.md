# sim-codec-algol

In one line: It lets you read and write values in familiar infix notation, the ordinary style where symbols sit between their operands.

## What it gives you

This is the everyday, math-like surface for SIM. It reads text written in the usual style -- values and operators laid out left to right, with the operator in the middle -- and turns it into a checked value the runtime can work with. It knows the ordinary precedence rules, so it groups things the way a reader expects without extra fuss. Going the other way, it writes any value back out in the same readable form, adding parentheses only where they are needed to keep the meaning clear. Because it covers the whole range of values rather than one narrow kind, anything the runtime can hold can travel through this surface and come back unchanged in meaning.

## Why you will be glad

- Expressions read the way most people already expect them to.
- Grouping follows standard precedence, so you write less punctuation by hand.
- Any value round-trips, so nothing is lost between reading and writing.

## Where it fits

This is one of the general-purpose reading and writing surfaces in the SIM codec family. It sits alongside the s-expression and JSON surfaces, offering a comfortable infix option for people who prefer that layout.
