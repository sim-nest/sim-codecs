use sim_kernel::Symbol;

/// Returns the packet shape symbol owned by the BRIDGE codec.
pub fn bridge_packet_shape_symbol() -> Symbol {
    Symbol::qualified("bridge", "Packet")
}
