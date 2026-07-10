# Lisp codec load-smoke (descriptor)

This is the lisp codec's load-smoke: it proves the `codec/lisp` reader is registered and
round-trips a symbol through the runtime, returning a fixed `codec-lisp-ok` sentinel. It is
a smoke descriptor, not a computation -- the lisp codec's real expressiveness is demonstrated
live by every runnable cookbook recipe (they all read `codec = "lisp"`) and by the
`codec/json` tagged-string recipe, which round-trips a cross-codec call to a computed `5`.
