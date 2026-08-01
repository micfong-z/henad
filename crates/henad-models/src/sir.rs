use henad_compute::chunked::{STATS_CHUNK, reduce_chunks};
use henad_core::grid::Grid2D;
use henad_core::grid_model::GridModel;
use henad_core::helpers::{extract_f32, f32_param, xorshift64};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatValue};

const S: u8 = 0;
const I: u8 = 1;
const R: u8 = 2;

pub const PALETTE: [[u8; 4]; 3] = [
    [0x00, 0x7A, 0xF5, 0xFF], // S - blue
    [0xE4, 0x37, 0x48, 0xFF], // I - red
    [0x80, 0x80, 0x80, 0xFF], // R - gray
];

pub struct SirGridModel;

pub struct SirParams {
    infection_rate: f32,
    recovery_rate: f32,
}

impl GridModel for SirGridModel {
    const NAME: &'static str = "SIR Epidemic";
    const ID: &'static str = "sir";
    const DESCRIPTION: &'static str = "Classic SIR compartmental model on a 2D grid with Moore neighborhood";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Susceptible", PALETTE[0]),
        StatDescriptor::new("Infected", PALETTE[1]),
        StatDescriptor::new("Recovered", PALETTE[2]),
    ];
    type Params = SirParams;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            f32_param("infection_rate", "Infection Rate", 0.3, 0.0, 1.0, Some(0.01)),
            f32_param("recovery_rate", "Recovery Rate", 0.05, 0.0, 1.0, Some(0.01)),
            f32_param(
                "initial_infected_pct",
                "Initial Infected %",
                0.01,
                0.0,
                1.0,
                Some(0.001),
            )
            .on_reload(),
        ]
    }

    fn from_params(params: &[ParamValue]) -> SirParams {
        SirParams {
            infection_rate: extract_f32(params, 2, 0.3),
            recovery_rate: extract_f32(params, 3, 0.05),
        }
    }

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64) {
        let initial_pct = extract_f32(params, 4, 0.01);
        let threshold = (initial_pct * u32::MAX as f32) as u32;
        for cell in grid.current_mut().iter_mut() {
            *rng = xorshift64(*rng);
            *cell = if ((*rng >> 32) as u32) < threshold { I } else { S };
        }
    }

    fn step_cell(cell: u8, neighbors: &[u8], params: &SirParams, rng: &mut u64) -> u8 {
        match cell {
            S => {
                let infected_count = neighbors.iter().filter(|&&n| n == I).count();
                if infected_count > 0 {
                    let prob_safe = (1.0 - params.infection_rate).powi(infected_count as i32);
                    *rng = xorshift64(*rng);
                    let rand_val = (*rng >> 33) as f32 / (u32::MAX >> 1) as f32;
                    if rand_val > prob_safe { I } else { S }
                } else {
                    S
                }
            }
            I => {
                *rng = xorshift64(*rng);
                let rand_val = (*rng >> 33) as f32 / (u32::MAX >> 1) as f32;
                if rand_val < params.recovery_rate { R } else { I }
            }
            _ => cell,
        }
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        let (s, i, r) = count_sir(grid.current());
        vec![
            StatValue::Scalar(s as f64),
            StatValue::Scalar(i as f64),
            StatValue::Scalar(r as f64),
        ]
    }
}

/// Count S/I/R in a single pass over a contiguous slice.
fn count_sir_seq(cells: &[u8]) -> (u64, u64, u64) {
    let (mut s, mut i, mut r) = (0u64, 0u64, 0u64);
    for &cell in cells {
        match cell {
            S => s += 1,
            I => i += 1,
            _ => r += 1,
        }
    }
    (s, i, r)
}

/// Count the S/I/R compartments.
fn count_sir(cells: &[u8]) -> (u64, u64, u64) {
    reduce_chunks(
        cells.len(),
        STATS_CHUNK,
        |r| count_sir_seq(&cells[r]),
        |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2),
        (0, 0, 0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::grid_engine::GridModelState;
    use henad_core::model::SimState as _;

    #[test]
    fn sir_population_conservation() {
        let params = vec![
            ParamValue::U32(100),
            ParamValue::U32(100),
            ParamValue::F32(0.3),
            ParamValue::F32(0.05),
            ParamValue::F32(0.01),
        ];
        let mut state = GridModelState::<SirGridModel>::from_params(&params);
        let pop = state.population();
        assert_eq!(pop, 10_000, "population should be 100x100");

        for _ in 0..100 {
            state.step();
        }
        assert_eq!(state.population(), pop, "S+I+R must remain constant over 100 steps");
    }

    #[test]
    fn sir_step_cell_transitions() {
        let params = SirParams {
            infection_rate: 1.0,
            recovery_rate: 0.0,
        };
        let mut rng = 42u64;
        // S with infected neighbor → always I (rate=1.0)
        assert_eq!(
            SirGridModel::step_cell(S, &[I, S, S, S, S, S, S, S], &params, &mut rng),
            I
        );
        // I with recovery_rate=0 → always I
        assert_eq!(
            SirGridModel::step_cell(I, &[S, S, S, S, S, S, S, S], &params, &mut rng),
            I
        );
        // R → always R
        assert_eq!(
            SirGridModel::step_cell(R, &[I, I, I, I, I, I, I, I], &params, &mut rng),
            R
        );
    }
}
