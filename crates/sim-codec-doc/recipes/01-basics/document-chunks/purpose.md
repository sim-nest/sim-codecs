# Document chunking codec

The document chunk operation decodes a Markdown source string and emits
heading-aligned chunks with their source byte offsets and heading paths. The
recipe runs the same chunker exposed by the `doc/chunk-heading` runtime
function.
