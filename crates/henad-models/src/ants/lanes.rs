use henad_compute::agent_lanes;

/// Value of `last_step` before an ant's first step. Momentum then has no direction to continue.
pub const NO_STEP: u8 = u8::MAX;

agent_lanes! {
    /// Lanes of an ant colony.
    ///
    /// Every lane is written in place. Ants never read one another, and each lane is touched only by
    /// the ant that owns the slot.
    pub struct AntLanes {
        read AntRead;
        chunk AntChunk;
        /// Column of the ant's cell.
        plain pos_x: f32 = 0.0,
        /// Row of the ant's cell.
        plain pos_y: f32 = 0.0,
        /// Last direction, encoded `(dx + 1) * 3 + (dy + 1)`, or [`NO_STEP`].
        plain last_step: u8 = NO_STEP,
        /// `1` while the ant carries food and `0` while it searches. The renderer reads this lane for
        /// the colour.
        plain has_food: u8 = 0,
        /// Amount the ant's next deposit adds to the best pheromone around it, and `0` once spent until
        /// a site grants a new reward.
        plain reward: f32 = 0.0,
    }
    color = has_food;
}
