use henad_compute::agent_lanes;

// --8<-- [start:lanes]
agent_lanes! {
    /// Lanes of a flock.
    ///
    /// Position and velocity are double buffered, since a boid reads its neighbours' current
    /// values while writing its own next values. Colour is written in place, as each boid writes
    /// only its own slot.
    pub struct BoidLanes {
        read BoidRead;
        chunk BoidChunk;
        /// Horizontal position in world units.
        dual pos_x / next_pos_x: f32,
        /// Vertical position in world units.
        dual pos_y / next_pos_y: f32,
        /// Horizontal velocity in world units per tick.
        dual vel_x / next_vel_x: f32,
        /// Vertical velocity in world units per tick.
        dual vel_y / next_vel_y: f32,
        /// Heading octant of the velocity, an index into [`HEADING_PALETTE`](super::HEADING_PALETTE).
        plain color: u8 = 0,
    }
    color = color;
}
// --8<-- [end:lanes]
