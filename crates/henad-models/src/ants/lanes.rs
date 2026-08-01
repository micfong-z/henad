use henad_compute::agent_lanes;

/// No step taken yet, so momentum has nothing to continue.
pub const NO_STEP: u8 = u8::MAX;

agent_lanes! {
    /// No double buffered lane. Ants never read one another, so every lane is touched only by the
    /// ant that owns the slot.
    pub struct AntLanes {
        read AntRead;
        chunk AntChunk;
        plain pos_x: f32 = 0.0,
        plain pos_y: f32 = 0.0,
        /// Last direction, encoded `(dx + 1) * 3 + (dy + 1)`, or [`NO_STEP`].
        plain last_step: u8 = NO_STEP,
        /// `0` searching, `1` carrying. Doubles as the render lane, so there is no colour lane.
        plain has_food: u8 = 0,
        plain reward: f32 = 0.0,
    }
    color = has_food;
}
