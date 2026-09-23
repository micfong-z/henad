use henad_compute::agent_lanes;

// --8<-- [start:lanes]
agent_lanes! {
    /// Node lanes for Virus on a Network.
    ///
    /// `state` is double buffered, since a node reads its neighbours' state while writing its own.
    /// `timer` is single buffered, as each node only touches its own.
    pub struct VirusLanes {
        read VirusRead;
        chunk VirusChunk;
        dual state / next_state: u8,
        plain timer: u32 = 0,
        plain pos_x: f32 = 0.0,
        plain pos_y: f32 = 0.0,
    }
    color = state;
}
// --8<-- [end:lanes]
