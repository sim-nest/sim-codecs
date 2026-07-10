# Binary frame codec (descriptor)

The binary codec encodes expressions as SLB8 byte frames: a symbol table, a value table, and
origin metadata. This recipe documents that frame structure; the codec round-trips bytes, not
an evaluable surface expression, so it is documented rather than run in the sandbox.
