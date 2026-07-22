# Codec positions and limits (descriptor)

This documents the codec protocol's encode positions (`eval`, `quote`, `data`, `pattern`) and
the resource limits a codec enforces while decoding. The recipe is a modeled
contract value because `sim-codec` defines the shared codec traits rather than a
loadable codec runtime lib.
