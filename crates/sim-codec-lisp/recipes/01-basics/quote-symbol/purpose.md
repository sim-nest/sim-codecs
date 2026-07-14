# Lisp codec load-smoke

This is the Lisp codec's load smoke: the recipe calls the Lisp CLI entrypoint
with an eval string, the entrypoint decodes that source through `codec/lisp`,
and the recipe encodes the returned symbol through the same codec.
