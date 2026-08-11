# Preserve TypeScript notation

Call `parse_module` for TypeScript or `parse_tsx` for TSX. The resulting tree's
`preserve_source` method returns the admitted input byte-for-byte, including
comments and whitespace, while extension nodes identify TypeScript notation.
