//! Models with bugs in them, for the tests that check a failed run is recorded and the rest of a sweep carries on.

use henad_core::authoring::model::grid_model::GridModel;
use henad_core::grid::Grid2D;
use henad_core::helpers::u32_param;
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatEntry, StatValue};
use std::sync::Arc;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::gpu::GpuContext;

/// Divides by `init_divisor` while it builds and by `divisor` in every step, so a sweep reaching 0 panics there.
pub struct DividesByParam;

impl GridModel for DividesByParam {
    const NAME: &'static str = "Divides By Param";
    const ID: &'static str = "divides_by_param";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF], [0xFF, 0xFF, 0xFF, 0xFF]];
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Cells", [0xFF, 0xFF, 0xFF, 0xFF])];
    type Params = u8;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("divisor", "Divisor", 1, 0, 4),
            u32_param("init_divisor", "Init divisor", 1, 0, 4),
        ]
    }

    fn from_params(params: &[ParamValue]) -> u8 {
        param_as_u8(params, 0)
    }

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], _rng: &mut u64) {
        grid.current_mut()[0] = 1 / param_as_u8(params, 1);
    }

    fn step_cell(cell: u8, _neighbors: &[u8], divisor: &u8, _rng: &mut u64) -> u8 {
        cell / *divisor
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(
            grid.current().iter().map(|&cell| f64::from(cell)).sum(),
        )]
    }
}

/// Counts every cell down by one a step from `countdown`, and reports the inverse of the first cell's count.
///
/// The inverse is not finite from the tick the count reaches 0.
pub struct InverseOfCountdown;

impl GridModel for InverseOfCountdown {
    const NAME: &'static str = "Inverse Of Countdown";
    const ID: &'static str = "inverse_of_countdown";
    const DESCRIPTION: &'static str = "A model whose stat stops being finite, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF], [0xFF, 0xFF, 0xFF, 0xFF]];
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Inverse", [0xFF, 0xFF, 0xFF, 0xFF])];
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![u32_param("countdown", "Countdown", 8, 0, 255)]
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], _rng: &mut u64) {
        grid.current_mut().fill(param_as_u8(params, 0));
    }

    fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
        cell.saturating_sub(1)
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(1.0 / f64::from(grid.current()[0]))]
    }
}

/// Returns parameter `index` of the model's own parameters as a `u8`, saturating at its largest value.
fn param_as_u8(params: &[ParamValue], index: usize) -> u8 {
    match params.get(index) {
        Some(&ParamValue::U32(value)) => u8::try_from(value).unwrap_or(u8::MAX),
        _ => 1,
    }
}

/// CPU state that steps as the state it wraps and refuses every action.
pub struct RefusesActions(pub Box<dyn SimState>);

impl RefusesActions {
    /// Returns `entry` with every CPU state it builds wrapped, so each refuses its actions.
    pub fn wrap(entry: ModelEntry) -> ModelEntry {
        entry.wrap_factory(|create| {
            Arc::new(
                move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| match create(
                    params, seed, gpu,
                )? {
                    ModelState::Cpu(state) => Ok(ModelState::Cpu(Box::new(Self(state)))),
                    ModelState::Gpu(state) => Ok(ModelState::Gpu(state)),
                },
            )
        })
    }
}

impl SimState for RefusesActions {
    fn step(&mut self) {
        self.0.step();
    }

    fn tick(&self) -> u64 {
        self.0.tick()
    }

    fn prepare_view(&mut self) {
        self.0.prepare_view();
    }

    fn stats(&self) -> Vec<StatEntry> {
        self.0.stats()
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.0.set_param(index, value)
    }

    fn act(&mut self, _index: usize) -> bool {
        false
    }

    fn population(&self) -> u64 {
        self.0.population()
    }

    fn heap_bytes(&self) -> usize {
        self.0.heap_bytes()
    }

    fn parallel_jobs(&self) -> Option<usize> {
        self.0.parallel_jobs()
    }
}
