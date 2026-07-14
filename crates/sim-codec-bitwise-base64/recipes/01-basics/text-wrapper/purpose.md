# Bitwise base64 wrapper

This codec wraps a bitwise frame in ASCII base64 for text-channel transport.
The recipe encodes a sample expression, wraps the vbits frame as text, decodes
the text back to bytes, and checks the recovered expression.
