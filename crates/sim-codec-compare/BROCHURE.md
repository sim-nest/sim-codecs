# sim-codec-compare

In one line: the honest scoreboard that tells you when the bit-packed wire format actually beats the plain one, and when it just wastes your time.

## What it gives you

A ready-to-run comparison between the two general-purpose SIM wire formats: the
byte-oriented one and the bit-packed one. It carries a built-in gallery of sample
data shaped like the things a real program stores -- small numbers, big numbers,
decimals, names, text, deep trees, wide records, and repeated blocks -- and for
each one it reports two plain facts: how many bytes each format takes, and how
long each takes to write and read. A single command prints the whole table. A set
of built-in checks pins the headline conclusions in place, so if a later change
quietly makes the bit-packed format worse, a test fails instead of a promise
silently breaking.

Here is the shape of a run (size is the packed format against the plain one, so
below one is smaller and better; effort is how much longer the packing takes):

    kind of data          size vs plain     write effort
    everyday records         0.6               1.1x
    lots of small numbers    0.5               1.5x
    repeated blocks          0.07 (packed)     1.2x
    plain text               1.0               9x

Read left to right that is the whole story: the packed format roughly halves
ordinary data for a little more work, shrinks repeated data to a small fraction,
and does nothing at all for plain text while costing many times the effort. Run
the report yourself for the current numbers on your own machine.

## Why you will be glad

Because it replaces opinion with a number. Instead of guessing whether the denser
format is worth it, you can see that ordinary records come out roughly forty
percent smaller, that repeated data can collapse to a fraction of its size, and
that plain text gains almost nothing while costing far more effort to pack. That
lets you pick the right format on purpose: the small, stable one for storage and
addressing, the fast one for hot paths and text-heavy traffic. You get the
recommendation and the evidence behind it in the same place.

## Where it fits

It sits beside the two codecs it measures and depends on both. It is a developer
and reviewer tool, not something a running program links against, so it stays out
of shipped builds. Reach for it whenever someone asks "is the dense format worth
it here" -- run the report, read the table, decide with facts.
