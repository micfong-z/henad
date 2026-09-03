use henad_compute::agent_lanes;

// --8<-- [start:lanes]
agent_lanes! {
    /// Position and velocity are double buffered, since a boid reads its neighbours' current
    /// values while writing its own next ones. Colour is not, each boid only writes its own slot.
    pub struct BoidLanes {
        read BoidRead;
        chunk BoidChunk;
        dual pos_x / next_pos_x: f32,
        dual pos_y / next_pos_y: f32,
        dual vel_x / next_vel_x: f32,
        dual vel_y / next_vel_y: f32,
        plain color: u8 = 0,
    }
    color = color;
}
// --8<-- [end:lanes]
