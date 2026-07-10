# Algol infix surface (descriptor)

The Algol codec parses an infix/prefix/postfix expression surface through a Pratt parser
into the shared `Expr` graph. That codec is not among the codecs loaded in the cookbook
sandbox eval stack (lisp and json), so this recipe documents the Algol surface rather than
round-tripping it live. The json tagged-string recipe shows a live non-lisp round-trip.
