# Evaluate a call through the JSON codec

The JSON codec decodes the tagged `call` expression -- `math/add` applied to `2`
and `3` -- the runtime evaluates it, and the codec encodes the result back as
JSON. A real computation over the tagged-expression form, not a sentinel string.
