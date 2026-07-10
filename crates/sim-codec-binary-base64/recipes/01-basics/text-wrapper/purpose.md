# Binary base64 wrapper (descriptor)

This codec wraps the binary SLB8 frame in ASCII base64 so a byte frame can travel over a text
channel. It is a transport wrapper over the binary frame, documented here rather than executed
in the sandbox (there is no surface expression to evaluate, only bytes to re-encode).
