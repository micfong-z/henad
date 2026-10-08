use henad_compute::agent_lanes;

// --8<-- [start:lanes]
agent_lanes! {
    /// Node lanes for Virus on a Network.
    ///
    /// `state` is double buffered, since a node reads its neighbours' state while writing its own state.
    /// `timer` is single buffered, as each node only touches its own timer.
    pub struct VirusLanes {
        read VirusRead;
        chunk VirusChunk;
        /// Infection state, one of `SUSCEPTIBLE`, `INFECTED` and `RESISTANT`.
        dual state / next_state: u8,
        /// Ticks since the node's last virus check, wrapping to 0 every `virus_check_frequency` ticks.
        plain timer: u32 = 0,
        /// Horizontal position in world units.
        plain pos_x: f32 = 0.0,
        /// Vertical position in world units.
        plain pos_y: f32 = 0.0,
    }
    color = state;
}
// --8<-- [end:lanes]
