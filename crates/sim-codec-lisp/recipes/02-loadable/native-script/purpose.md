# Decode a native-loaded script

The setup form is ordinary Lisp source. The native loader test builds the
codec as a dynamic library, loads `codec/lisp`, and decodes this same form
through the loaded codec export.
