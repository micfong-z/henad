use std::mem::swap;

use henad_core::{
    model::SimState,
    params::ParamValue,
    spatial_hash::SpatialHash,
    view::{PointView, StatDescriptor, StatEntry, StatsHistory},
};

pub const PALETTE: [[u8; 4]; 3] = [
    [0xE4, 0x37, 0x48, 0xFF], // Max speed - red
    [0xFF, 0xC1, 0x07, 0xFF], // Avg speed - yellow
    [0x00, 0x7A, 0xF5, 0xFF], // Min speed - blue
];

pub struct BoidsState {
    pub(crate) pos_x: Vec<f32>,
    pub(crate) pos_y: Vec<f32>,
    pub(crate) vel_x: Vec<f32>,
    pub(crate) vel_y: Vec<f32>,
    pub(crate) next_pos_x: Vec<f32>,
    pub(crate) next_pos_y: Vec<f32>,
    pub(crate) next_vel_x: Vec<f32>,
    pub(crate) next_vel_y: Vec<f32>,
    pub(crate) hash: SpatialHash,
    pub(crate) world_w: f32,
    pub(crate) world_h: f32,
    pub(crate) num_boids: u32,
    pub(crate) num_boids_inv: f32,
    // Hot params (set_param indices 3-9 → return true)
    pub(crate) visual_range: f32,
    pub(crate) protected_range: f32,
    pub(crate) separation_factor: f32,
    pub(crate) alignment_factor: f32,
    pub(crate) cohesion_factor: f32,
    pub(crate) max_speed: f32,
    pub(crate) min_speed: f32,
    pub(crate) tick: u64,
    pub(crate) avg_speed: f32,
    pub(crate) history: StatsHistory,
    pub(crate) rng_state: u64,
}

const STATS_HISTORY_CAPACITY: usize = 10_000;

impl BoidsState {
    pub fn from_params(params: &[ParamValue]) -> Self {
        let num_boids = match params.first() {
            Some(ParamValue::U32(v)) => *v,
            _ => 50_000,
        };
        let world_w = match params.get(1) {
            Some(ParamValue::F32(v)) => *v,
            _ => 1_000.0,
        };
        let world_h = match params.get(2) {
            Some(ParamValue::F32(v)) => *v,
            _ => 1_000.0,
        };
        let visual_range = match params.get(3) {
            Some(ParamValue::F32(v)) => *v,
            _ => 50.0,
        };
        let protected_range = match params.get(4) {
            Some(ParamValue::F32(v)) => *v,
            _ => 8.0,
        };
        let separation_factor = match params.get(5) {
            Some(ParamValue::F32(v)) => *v,
            _ => 0.05,
        };
        let alignment_factor = match params.get(6) {
            Some(ParamValue::F32(v)) => *v,
            _ => 0.05,
        };
        let cohesion_factor = match params.get(7) {
            Some(ParamValue::F32(v)) => *v,
            _ => 0.0005,
        };
        let max_speed = match params.get(8) {
            Some(ParamValue::F32(v)) => *v,
            _ => 15.0,
        };
        let min_speed = match params.get(9) {
            Some(ParamValue::F32(v)) => *v,
            _ => 3.0,
        };

        let pos_x = vec![0.0; num_boids as usize];
        let pos_y = vec![0.0; num_boids as usize];
        let vel_x = vec![0.0; num_boids as usize];
        let vel_y = vec![0.0; num_boids as usize];
        let next_pos_x = vec![0.0; num_boids as usize];
        let next_pos_y = vec![0.0; num_boids as usize];
        let next_vel_x = vec![0.0; num_boids as usize];
        let next_vel_y = vec![0.0; num_boids as usize];

        let hash = SpatialHash::new(visual_range, world_w, world_h);
        let history = StatsHistory::new(
            vec![StatDescriptor {
                label: "Average Speed",
                color: PALETTE[1],
            }],
            STATS_HISTORY_CAPACITY,
        );
        let num_boids_inv = 1.0 / num_boids as f32;

        let mut state = Self {
            pos_x,
            pos_y,
            vel_x,
            vel_y,
            next_pos_x,
            next_pos_y,
            next_vel_x,
            next_vel_y,
            hash,
            world_w,
            world_h,
            num_boids,
            visual_range,
            protected_range,
            separation_factor,
            alignment_factor,
            cohesion_factor,
            max_speed,
            min_speed,
            tick: 0,
            avg_speed: 0.0,
            history,
            rng_state: 0xDEAD_BEEF_CAFE_1234,
            num_boids_inv,
        };

        state.initialize();
        state.record_history();

        state
    }

    fn initialize(&mut self) {
        let speed = 0.5 * (self.min_speed + self.max_speed);
        let inv_u32_range = 1.0f32 / (u32::MAX as f32 + 1.0);
        let mut rng = self.rng_state;

        for i in 0..self.num_boids as usize {
            rng = xorshift64(rng);
            let rx = ((rng >> 32) as u32) as f32 * inv_u32_range;

            rng = xorshift64(rng);
            let ry = ((rng >> 32) as u32) as f32 * inv_u32_range;

            self.pos_x[i] = rx * self.world_w;
            self.pos_y[i] = ry * self.world_h;

            rng = xorshift64(rng);
            let ra = ((rng >> 32) as u32) as f32 * inv_u32_range;
            let angle = ra * std::f32::consts::TAU;
            self.vel_x[i] = angle.cos() * speed;
            self.vel_y[i] = angle.sin() * speed;
        }
        self.rng_state = rng;
    }

    pub(crate) fn record_history(&mut self) {
        self.history.push(&[self.avg_speed as f64]);
    }

    pub fn swap_buffers(&mut self) {
        swap(&mut self.pos_x, &mut self.next_pos_x);
        swap(&mut self.pos_y, &mut self.next_pos_y);
        swap(&mut self.vel_x, &mut self.next_vel_x);
        swap(&mut self.vel_y, &mut self.next_vel_y);
    }
}

/// Fast xorshift64 RNG.
#[inline]
pub fn xorshift64(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

impl SimState for BoidsState {
    fn step(&mut self) {
        super::step::step(self);
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn point_view(&self) -> Option<PointView<'_>> {
        Some(PointView {
            pos_x: &self.pos_x,
            pos_y: &self.pos_y,
            world_w: self.world_w,
            world_h: self.world_h,
            palette: &[0xFF, 0xA0, 0x00, 0xFF],
        })
    }

    fn stats(&self) -> Vec<henad_core::view::StatEntry> {
        vec![StatEntry {
            label: "Average Speed",
            value: self.avg_speed as f64,
            color: PALETTE[1],
        }]
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        match (index, value) {
            (3, ParamValue::F32(v)) => {
                self.visual_range = *v;
                self.hash.rebuild_with_cell_size(self.visual_range, &self.pos_x, &self.pos_y);
                true
            }
            (4, ParamValue::F32(v)) => {
                self.protected_range = *v;
                true
            }
            (5, ParamValue::F32(v)) => {
                self.separation_factor = *v;
                true
            }
            (6, ParamValue::F32(v)) => {
                self.alignment_factor = *v;
                true
            }
            (7, ParamValue::F32(v)) => {
                self.cohesion_factor = *v;
                true
            }
            (8, ParamValue::F32(v)) => {
                self.max_speed = *v;
                true
            }
            (9, ParamValue::F32(v)) => {
                self.min_speed = *v;
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> ParamValue {
        match index {
            0 => ParamValue::U32(self.num_boids),
            1 => ParamValue::F32(self.world_w),
            2 => ParamValue::F32(self.world_h),
            3 => ParamValue::F32(self.visual_range),
            4 => ParamValue::F32(self.protected_range),
            5 => ParamValue::F32(self.separation_factor),
            6 => ParamValue::F32(self.alignment_factor),
            7 => ParamValue::F32(self.cohesion_factor),
            8 => ParamValue::F32(self.max_speed),
            9 => ParamValue::F32(self.min_speed),
            _ => ParamValue::U32(0),
        }
    }

    fn population(&self) -> u64 {
        self.num_boids as u64
    }

    fn stats_history(&self) -> &StatsHistory {
        &self.history
    }

    fn resize_history(&mut self, capacity: usize) {
        self.history.resize(capacity);
    }

    fn heap_bytes(&self) -> usize {
        8 * 4 * self.pos_x.capacity() + self.hash.heap_bytes() + self.history.heap_bytes()
    }
}
