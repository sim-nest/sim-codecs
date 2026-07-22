# Binary base64 wrapper

This codec wraps a binary SLB8 frame in ASCII base64 so byte frames can travel
over a text channel. The recipe encodes a sample expression, base64-wraps the
frame, decodes the text back to bytes, and checks the recovered expression.
