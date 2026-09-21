use henad_compute::agent_lanes;

// --8<-- [start:lanes]
agent_lanes! {
    /// Node lanes for Team Assembly.
    ///
    /// Every lane is written in place, since the node pass is empty and the global pass is sequential.
    ///
    /// `spawn_tick` is the tick at which the node was spawned, and `team_tick` the tick at which it last joined a team.
    /// A tick in these lanes is the tick count at the end of the step.
    /// The first step writes 1, and setup nodes have 0.
    /// `mark` is the tick at which the node was last listed as a collaborator candidate, so each is listed once a tick.
    pub struct TeamLanes {
        read TeamRead;
        chunk TeamChunk;
        plain spawn_tick: u64 = 0,
        plain team_tick: u64 = 0,
        plain mark: u64 = 0,
        plain color: u8 = 0,
        plain pos_x: f32 = 0.0,
        plain pos_y: f32 = 0.0,
    }
    color = color;
}
// --8<-- [end:lanes]
