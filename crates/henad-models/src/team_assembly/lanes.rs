use henad_compute::agent_lanes;

// --8<-- [start:lanes]
agent_lanes! {
    /// Node lanes for Team Assembly.
    ///
    /// Every lane is written in place, since the node pass is empty and the global pass is sequential.
    ///
    /// A tick in these lanes is the tick count at the end of the step.
    /// The first step writes 1, and setup nodes have 0.
    pub struct TeamLanes {
        read TeamRead;
        chunk TeamChunk;
        /// Tick at which the node was spawned.
        plain spawn_tick: u64 = 0,
        /// Tick at which the node last joined a team.
        plain team_tick: u64 = 0,
        /// Tick at which the node was last listed as a collaborator candidate, so it is listed at most once a tick.
        plain mark: u64 = 0,
        /// Role in the last team, one of `IDLE`, `INCUMBENT` and `NEWCOMER`, set by `prepare_view`.
        plain color: u8 = 0,
        /// Horizontal position in world units, or `NaN` in a retired slot.
        plain pos_x: f32 = 0.0,
        /// Vertical position in world units, or `NaN` in a retired slot.
        plain pos_y: f32 = 0.0,
    }
    color = color;
}
// --8<-- [end:lanes]
