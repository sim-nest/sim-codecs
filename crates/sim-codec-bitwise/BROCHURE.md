# sim-codec-bitwise

In one line: It writes any value in the smallest, most predictable string of bits, the same value always giving the same result.

## What it gives you

This is the tightest packing in the family. Where the ordinary binary form works a byte at a time, this one works bit by bit, so no space is wasted rounding up to whole bytes. Lengths and numbers are written using only as many bits as they truly need, and whole numbers of any sign shrink to their bare significant bits. The result is not just small but canonical: one value always produces one exact string of bits, and one string always reads back to one value. That steadiness makes it a natural fit when you want to name or look something up by its content, since identical values always share an identical form. Measured on real data it comes out about forty to fifty percent smaller than the byte format on everyday records and numbers, for a little more packing work; plain text is the one case that gains nothing, so reach for the byte format there. As with the other byte formats, broken input is refused rather than followed.

## Why you will be glad

- Values pack into the fewest bits, tighter than the byte-aligned form.
- The same value always yields the same bytes, so content can be compared or addressed directly.
- Malformed frames fail closed instead of being acted upon.

## Where it fits

This is the canonical, minimal member of the SIM codec family, a sibling of the binary format. It is the form to choose when exact, repeatable, content-addressable bytes matter most.
