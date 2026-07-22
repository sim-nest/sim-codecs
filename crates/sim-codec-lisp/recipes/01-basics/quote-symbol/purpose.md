# Lisp codec quoted symbol

This recipe sends a quoted symbol through the Lisp CLI entrypoint. The entrypoint
decodes the source through `codec/lisp`, evaluates the quote form without looking
up the symbol as a binding, and encodes the returned symbol through the same
codec.
