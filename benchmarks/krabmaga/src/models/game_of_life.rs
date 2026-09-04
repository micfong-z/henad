//! Conway's Game of Life, B3/S23 on a Moore torus, updated synchronously.
//!
//! Follows the shape of krABMaga's own `forestfire` example. One agent sweeps the whole grid,
//! reading the field's read buffer and writing its write buffer, and `Schedule::step` swaps the
//! two once every cell has been written. No cell sees a neighbour that has already moved on.

use std::any::Any;

use krabmaga::engine::agent::Agent;
use krabmaga::engine::fields::dense_number_grid_2d::DenseNumberGrid2D;
use krabmaga::engine::fields::field::Field;
use krabmaga::engine::location::Int2D;
use krabmaga::engine::schedule::Schedule;
use krabmaga::engine::state::State;
use krabmaga::rand::{Rng, SeedableRng};
use krabmaga::rand_pcg::Pcg64;

use crate::models::wrap;

const DEAD: u8 = 0;
const ALIVE: u8 = 1;

#[derive(Clone)]
struct Sweeper;

impl Agent for Sweeper {
    fn step(&mut self, state: &mut dyn State) {
        let model = state.as_any().downcast_ref::<GameOfLife>().expect("Game of Life state");
        let (width, height) = (model.width, model.height);

        for x in 0..width {
            for y in 0..height {
                let mut alive = 0u8;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let loc = Int2D {
                            x: wrap(x + dx, width),
                            y: wrap(y + dy, height),
                        };
                        alive += model.cells.get_value(&loc).unwrap_or(DEAD);
                    }
                }
                let here = Int2D { x, y };
                let cell = model.cells.get_value(&here).unwrap_or(DEAD);
                let next = match (cell, alive) {
                    (ALIVE, 2..=3) | (DEAD, 3) => ALIVE,
                    _ => DEAD,
                };
                model.cells.set_value_location(next, &here);
            }
        }
    }
}

pub struct GameOfLife {
    pub width: i32,
    pub height: i32,
    cells: DenseNumberGrid2D<u8>,
    density: f64,
    /// An exact set of live cells for a gate scenario, in place of the random fill.
    live: Option<Vec<(i32, i32)>>,
    seed: u64,
}

impl GameOfLife {
    pub fn new(width: i32, height: i32, density: f64, seed: u64) -> Self {
        GameOfLife {
            width,
            height,
            cells: DenseNumberGrid2D::new(width, height),
            density,
            live: None,
            seed,
        }
    }

    pub fn with_live(width: i32, height: i32, live: &[(i32, i32)]) -> Self {
        let mut model = GameOfLife::new(width, height, 0.0, 0);
        model.live = Some(live.to_vec());
        model
    }

    pub fn population(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Before the first step the grid still sits in the write buffer, since `Schedule::step` is
    /// what publishes it.
    fn cell(&self, x: i32, y: i32) -> u8 {
        let loc = Int2D { x, y };
        self.cells
            .get_value(&loc)
            .or_else(|| self.cells.get_value_unbuffered(&loc))
            .unwrap_or(DEAD)
    }

    /// Rows ascending in y, each row ascending in x, as the fixture format wants.
    pub fn bitmap(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| (0..self.width).map(|x| char::from(b'0' + self.cell(x, y))).collect())
            .collect()
    }
}

impl State for GameOfLife {
    fn init(&mut self, schedule: &mut Schedule) {
        let mut rng = Pcg64::seed_from_u64(self.seed);
        for x in 0..self.width {
            for y in 0..self.height {
                let state = match &self.live {
                    Some(live) => {
                        if live.contains(&(x, y)) {
                            ALIVE
                        } else {
                            DEAD
                        }
                    }
                    None => {
                        if rng.random::<f64>() < self.density {
                            ALIVE
                        } else {
                            DEAD
                        }
                    }
                };
                self.cells.set_value_location(state, &Int2D { x, y });
            }
        }
        schedule.schedule_repeating(Box::new(Sweeper), 0.0, 0);
    }

    fn update(&mut self, _step: u64) {
        self.cells.lazy_update();
    }

    fn reset(&mut self) {
        self.cells = DenseNumberGrid2D::new(self.width, self.height);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_state(&self) -> &dyn State {
        self
    }

    fn as_state_mut(&mut self) -> &mut dyn State {
        self
    }
}
