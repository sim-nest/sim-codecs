# sim-test-support

In one line: It is the shared set of helpers the SIM formats use to test that they read and write values correctly.

## What it gives you

Every format in this family needs to prove the same thing: that a value written out and read back comes home unchanged. Rather than each one carrying its own copy of that checking machinery, this holds a single shared set of helpers they all draw on. It offers a ready-made practice runtime to test against, a round-trip helper that sends a value out and back and confirms it matches, and small search helpers for inspecting results. Keeping this in one place means the checks stay consistent across every format, and a fix or improvement lands everywhere at once. It is used only while testing, so it never becomes part of anything shipped, and it stays deliberately light so it never tangles the formats it serves.

## Why you will be glad

- Every format is checked the same consistent way.
- A single round-trip helper proves values survive writing and reading intact.
- One shared home means fixes reach all the formats at once.

## Where it fits

This is the shared testing helper for the SIM codec family. It sits to one side of the formats themselves, giving each of them a common, trustworthy way to confirm it reads and writes values correctly.
