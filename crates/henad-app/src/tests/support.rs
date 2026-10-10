//! Models and helpers the app's tests of replaced model code share.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use henad_compute::entry::{ModelEntry, ModelSet, ModelState, register_grid_model};
use henad_compute::fault::Fault;
use henad_compute::gpu::GpuContext;
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::grid::Grid2D;
use henad_core::helpers::f32_param;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::provenance::ModelCode;
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatValue};

use crate::state::AppState;

/// SIR as an edit leaves its declarations: an infection rate capped at 0.5 with another default, no recovery rate, a
/// new vaccination rate, one stat fewer, and a new action ahead of the outbreak.
pub struct EditedSir;

impl GridModel for EditedSir {
    const NAME: &'static str = "SIR Edited";
    const ID: &'static str = "sir";
    const DESCRIPTION: &'static str = "SIR after an edit of its declarations";
    const PALETTE: &'static [[u8; 4]] = &[[0, 0, 255, 255], [255, 0, 0, 255]];
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Susceptible", [0, 0, 255, 255]),
        StatDescriptor::new("Infected", [255, 0, 0, 255]),
    ];
    const ACTIONS: &'static [ActionDescriptor] = &[
        ActionDescriptor::new("vaccinate", "Vaccinate"),
        ActionDescriptor::new("seed_outbreak", "Seed outbreak"),
    ];
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            f32_param("infection_rate", "Infection Rate", 0.2, 0.0, 0.5, Some(0.01)),
            f32_param("initial_infected_pct", "Initial Infected", 0.01, 0.0, 1.0, Some(0.001))
                .on_reload()
                .percent(),
            f32_param("vaccination_rate", "Vaccination Rate", 0.0, 0.0, 1.0, Some(0.01)),
        ]
    }

    fn from_params(_params: &[ParamValue]) -> Self::Params {}

    fn init(_grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

    fn step_cell(cell: u8, _neighbors: &[u8], _params: &Self::Params, _rng: &mut u64) -> u8 {
        cell
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(grid.current().len() as f64), StatValue::Scalar(0.0)]
    }
}

/// Returns the entry of [`EditedSir`], registered under SIR's id.
pub fn edited_sir() -> ModelEntry {
    register_grid_model::<EditedSir>()
}

/// Returns a set holding `entry` alone.
pub fn set_of(entry: ModelEntry) -> ModelSet {
    let mut models = ModelSet::new(henad_core::build_info!());
    models.insert(entry).expect("an id in the grammar");
    models
}

/// Returns a counter of builds at zero.
pub fn counter() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

/// Returns `entry` under the runtime code `text`, with each of its builds counted in `builds`.
pub fn recoded(entry: &ModelEntry, text: &str, builds: &Arc<AtomicUsize>) -> ModelEntry {
    let builds = Arc::clone(builds);
    entry
        .clone()
        .__with_code(ModelCode::new("henad-app-test", text.to_owned()))
        .wrap_factory(move |inner| {
            Arc::new(
                move |params: &[ParamValue],
                      seed: Option<u64>,
                      gpu: Option<&GpuContext>|
                      -> Result<ModelState, Fault> {
                    builds.fetch_add(1, Ordering::SeqCst);
                    inner(params, seed, gpu)
                },
            )
        })
}

/// Steps the loaded model once and takes its snapshots until one is past `tick`, or panics after ten seconds.
pub fn step_past(app: &mut AppState, tick: u64) {
    let thread = app.sim_thread.as_mut().expect("a model is loaded");
    thread.step_once();
    for _ in 0..1000 {
        if let Some(snapshot) = thread.take_snapshot()
            && snapshot.tick > tick
        {
            app.snapshot = Some(snapshot);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the loaded model never passed tick {tick}");
}
