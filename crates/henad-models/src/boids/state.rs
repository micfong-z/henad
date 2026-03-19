use std::mem::swap;

use henad_core::helpers::{extract_f32, extract_u32, xorshift64};
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::spatial_hash::SpatialHash;
use henad_core::view::{PointView, StatEntry, StatValue};

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
    pub(crate) visual_range: f32,
    pub(crate) protected_range: f32,
    pub(crate) separation_factor: f32,
    pub(crate) alignment_factor: f32,
    pub(crate) cohesion_factor: f32,
    pub(crate) max_speed: f32,
    pub(crate) min_speed: f32,
    pub(crate) tick: u64,
    pub(crate) avg_speed: f32,
    pub(crate) avg_vel_x: f32,
    pub(crate) avg_vel_y: f32,
    pub(crate) rng_state: u64,
}

impl BoidsState {
    pub fn from_params(params: &[ParamValue]) -> Self {
        let num_boids = extract_u32(params, 0, 50_000);
        let world_w = extract_f32(params, 1, 1_000.0);
        let world_h = extract_f32(params, 2, 1_000.0);
        let visual_range = extract_f32(params, 3, 50.0);
        let protected_range = extract_f32(params, 4, 8.0);
        let separation_factor = extract_f32(params, 5, 0.05);
        let alignment_factor = extract_f32(params, 6, 0.05);
        let cohesion_factor = extract_f32(params, 7, 0.0005);
        let max_speed = extract_f32(params, 8, 15.0);
        let min_speed = extract_f32(params, 9, 3.0);

        let n = num_boids as usize;
        let hash = SpatialHash::new(visual_range, world_w, world_h);
        let num_boids_inv = 1.0 / num_boids as f32;

        let mut state = Self {
            pos_x: vec![0.0; n],
            pos_y: vec![0.0; n],
            vel_x: vec![0.0; n],
            vel_y: vec![0.0; n],
            next_pos_x: vec![0.0; n],
            next_pos_y: vec![0.0; n],
            next_vel_x: vec![0.0; n],
            next_vel_y: vec![0.0; n],
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
            avg_vel_x: 0.0,
            avg_vel_y: 0.0,
            rng_state: 0xDEAD_BEEF_CAFE_1234,
            num_boids_inv,
        };

        state.initialize();
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

    pub fn swap_buffers(&mut self) {
        swap(&mut self.pos_x, &mut self.next_pos_x);
        swap(&mut self.pos_y, &mut self.next_pos_y);
        swap(&mut self.vel_x, &mut self.next_vel_x);
        swap(&mut self.vel_y, &mut self.next_vel_y);
    }
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

    fn stats(&self) -> Vec<StatEntry> {
        vec![
            StatEntry {
                label: "Average Speed",
                value: StatValue::Scalar(self.avg_speed as f64),
                color: PALETTE[1],
            },
            StatEntry {
                label: "Average Velocity",
                value: StatValue::Vector2D {
                    x: self.avg_vel_x as f64,
                    y: self.avg_vel_y as f64,
                },
                color: PALETTE[2],
            },
        ]
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        match (index, value) {
            (3, ParamValue::F32(v)) => {
                self.visual_range = *v;
                self.hash
                    .rebuild_with_cell_size(self.visual_range, &self.pos_x, &self.pos_y);
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

    fn population(&self) -> u64 {
        self.num_boids as u64
    }

    fn heap_bytes(&self) -> usize {
        8 * 4 * self.pos_x.capacity() + self.hash.heap_bytes()
    }
}
