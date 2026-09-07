//! SIR on a Moore torus, updated synchronously.
//!
//! A susceptible cell with `k` infected neighbours catches it with probability `1 - (1 - beta)^k`,
//! an infected cell recovers with probability `gamma`, and recovery is permanent. Same sweeping
//! shape as Game of Life, so no cell sees a neighbour that has already changed this tick.

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

const SUSCEPTIBLE: u8 = 0;
const INFECTED: u8 = 1;
const RECOVERED: u8 = 2;

/// The draws live on the agent, so one sweep's stream continues into the next.
#[derive(Clone)]
struct Sweeper {
    rng: Pcg64,
}

impl Agent for Sweeper {
    fn step(&mut self, state: &mut dyn State) {
        let model = state.as_any().downcast_ref::<Sir>().expect("SIR state");
        let (width, height) = (model.width, model.height);

        for x in 0..width {
            for y in 0..height {
                let here = Int2D { x, y };
                let cell = model.cells.get_value(&here).unwrap_or(SUSCEPTIBLE);
                let next = match cell {
                    SUSCEPTIBLE => {
                        let mut infected = 0i32;
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                if dx == 0 && dy == 0 {
                                    continue;
                                }
                                let loc = Int2D {
                                    x: wrap(x + dx, width),
                                    y: wrap(y + dy, height),
                                };
                                if model.cells.get_value(&loc) == Some(INFECTED) {
                                    infected += 1;
                                }
                            }
                        }
                        let catches = infected > 0
                            && self.rng.random::<f64>() < 1.0 - (1.0 - model.infection_rate).powi(infected);
                        if catches {
                            INFECTED
                        } else {
                            SUSCEPTIBLE
                        }
                    }
                    INFECTED => {
                        if self.rng.random::<f64>() < model.recovery_rate {
                            RECOVERED
                        } else {
                            INFECTED
                        }
                    }
                    _ => RECOVERED,
                };
                model.cells.set_value_location(next, &here);
            }
        }
    }
}

pub struct Sir {
    pub width: i32,
    pub height: i32,
    cells: DenseNumberGrid2D<u8>,
    infection_rate: f64,
    recovery_rate: f64,
    initial_infected_pct: f64,
    seed: u64,
}

impl Sir {
    pub fn new(
        width: i32,
        height: i32,
        infection_rate: f64,
        recovery_rate: f64,
        initial_infected_pct: f64,
        seed: u64,
    ) -> Self {
        Sir {
            width,
            height,
            cells: DenseNumberGrid2D::new(width, height),
            infection_rate,
            recovery_rate,
            initial_infected_pct,
            seed,
        }
    }

    pub fn population(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Susceptible, infected and recovered totals.
    ///
    /// Before the first step the grid still sits in the write buffer, since `Schedule::step` is
    /// what publishes it.
    pub fn counts(&self) -> [u64; 3] {
        let mut totals = [0u64; 3];
        for x in 0..self.width {
            for y in 0..self.height {
                let loc = Int2D { x, y };
                let cell = self
                    .cells
                    .get_value(&loc)
                    .or_else(|| self.cells.get_value_unbuffered(&loc))
                    .unwrap_or(SUSCEPTIBLE);
                totals[cell as usize] += 1;
            }
        }
        totals
    }
}

impl State for Sir {
    fn init(&mut self, schedule: &mut Schedule) {
        let mut rng = Pcg64::seed_from_u64(self.seed);
        for x in 0..self.width {
            for y in 0..self.height {
                let state = if rng.random::<f64>() < self.initial_infected_pct {
                    INFECTED
                } else {
                    SUSCEPTIBLE
                };
                self.cells.set_value_location(state, &Int2D { x, y });
            }
        }
        // A stream of its own, so the initial fill and the steps never share draws.
        schedule.schedule_repeating(
            Box::new(Sweeper {
                rng: Pcg64::seed_from_u64(self.seed ^ 0x9E37_79B9_7F4A_7C15),
            }),
            0.0,
            0,
        );
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
