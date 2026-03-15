use henad_core::grid::Grid2D;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::{GridView, StatDescriptor, StatEntry, StatsHistory};

/// Cell states for SIR.
pub const SUSCEPTIBLE: u8 = 0;
pub const INFECTED: u8 = 1;
pub const RECOVERED: u8 = 2;

/// RGBA palette: S=blue, I=red, R=gray.
pub const PALETTE: [[u8; 4]; 3] = [
    [70, 130, 230, 255],  // S - blue
    [230, 60, 60, 255],   // I - red
    [128, 128, 128, 255], // R - gray
];

const STATS_HISTORY_CAPACITY: usize = 10_000;

pub struct SirState {
    pub(crate) grid: Grid2D<u8>,
    pub(crate) tick: u64,
    pub(crate) infection_rate: f32,
    pub(crate) recovery_rate: f32,
    pub(crate) initial_infected_pct: f32,
    pub(crate) count_s: u64,
    pub(crate) count_i: u64,
    pub(crate) count_r: u64,
    pub(crate) rng_state: u64,
    pub(crate) history: StatsHistory,
}

impl SirState {
    pub fn from_params(params: &[ParamValue]) -> Self {
        let width = match params.first() {
            Some(ParamValue::U32(v)) => *v,
            _ => 1024,
        };
        let height = match params.get(1) {
            Some(ParamValue::U32(v)) => *v,
            _ => 1024,
        };
        let infection_rate = match params.get(2) {
            Some(ParamValue::F32(v)) => *v,
            _ => 0.3,
        };
        let recovery_rate = match params.get(3) {
            Some(ParamValue::F32(v)) => *v,
            _ => 0.05,
        };
        let initial_infected_pct = match params.get(4) {
            Some(ParamValue::F32(v)) => *v,
            _ => 0.01,
        };

        let history = StatsHistory::new(
            vec![
                StatDescriptor {
                    label: "Susceptible",
                    color: PALETTE[0],
                },
                StatDescriptor {
                    label: "Infected",
                    color: PALETTE[1],
                },
                StatDescriptor {
                    label: "Recovered",
                    color: PALETTE[2],
                },
            ],
            STATS_HISTORY_CAPACITY,
        );

        let mut state = Self {
            grid: Grid2D::new(width, height),
            tick: 0,
            infection_rate,
            recovery_rate,
            initial_infected_pct,
            count_s: 0,
            count_i: 0,
            count_r: 0,
            rng_state: 0xDEAD_BEEF_CAFE_1234,
            history,
        };
        state.initialize();
        // Record initial state at tick 0
        state.record_history();
        state
    }

    fn initialize(&mut self) {
        let len = self.grid.len();
        let cells = self.grid.current_mut();
        let threshold = (self.initial_infected_pct * u32::MAX as f32) as u32;

        let mut rng = self.rng_state;
        let mut count_i: u64 = 0;

        for cell in cells.iter_mut() {
            rng = xorshift64(rng);
            let rand_val = (rng >> 32) as u32;
            if rand_val < threshold {
                *cell = INFECTED;
                count_i += 1;
            } else {
                *cell = SUSCEPTIBLE;
            }
        }

        self.rng_state = rng;
        self.count_i = count_i;
        self.count_s = len as u64 - count_i;
        self.count_r = 0;
    }

    pub(crate) fn record_history(&mut self) {
        self.history.push(&[
            self.count_s as f64,
            self.count_i as f64,
            self.count_r as f64,
        ]);
    }
}

impl SimState for SirState {
    fn step(&mut self) {
        super::step::step(self);
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        Some(GridView {
            width: self.grid.width(),
            height: self.grid.height(),
            cells: self.grid.current(),
            palette: &PALETTE,
        })
    }

    fn stats(&self) -> Vec<StatEntry> {
        vec![
            StatEntry {
                label: "Susceptible",
                value: self.count_s as f64,
                color: PALETTE[0],
            },
            StatEntry {
                label: "Infected",
                value: self.count_i as f64,
                color: PALETTE[1],
            },
            StatEntry {
                label: "Recovered",
                value: self.count_r as f64,
                color: PALETTE[2],
            },
        ]
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        match (index, value) {
            (2, ParamValue::F32(v)) => {
                self.infection_rate = *v;
                true
            }
            (3, ParamValue::F32(v)) => {
                self.recovery_rate = *v;
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> ParamValue {
        match index {
            0 => ParamValue::U32(self.grid.width()),
            1 => ParamValue::U32(self.grid.height()),
            2 => ParamValue::F32(self.infection_rate),
            3 => ParamValue::F32(self.recovery_rate),
            4 => ParamValue::F32(self.initial_infected_pct),
            _ => ParamValue::U32(0),
        }
    }

    fn population(&self) -> u64 {
        self.count_s + self.count_i + self.count_r
    }

    fn stats_history(&self) -> &StatsHistory {
        &self.history
    }

    fn resize_history(&mut self, capacity: usize) {
        self.history.resize(capacity);
    }

    fn heap_bytes(&self) -> usize {
        self.grid.heap_bytes() + self.history.heap_bytes()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sir_state_conservation() {
        let params = vec![
            ParamValue::U32(100),
            ParamValue::U32(100),
            ParamValue::F32(0.3),
            ParamValue::F32(0.05),
            ParamValue::F32(0.01),
        ];
        let mut state = SirState::from_params(&params);
        let pop = state.population();
        assert_eq!(pop, 10_000, "population should be 100x100");

        for _ in 0..100 {
            state.step();
        }
        assert_eq!(
            state.population(),
            pop,
            "S+I+R must remain constant over 100 steps"
        );
    }

    #[test]
    fn xorshift64_no_zero() {
        let mut s = 1u64;
        for _ in 0..1000 {
            s = xorshift64(s);
            assert_ne!(s, 0, "xorshift64 should not produce 0");
        }
    }
}
