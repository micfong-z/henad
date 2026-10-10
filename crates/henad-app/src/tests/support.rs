//! Models and helpers the app's tests share: replaced model code, held builds and steps, and the widgets of a frame.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use henad_compute::entry::{ModelEntry, ModelSet, ModelState, register_grid_model};
use henad_compute::fault::Fault;
use henad_compute::gpu::GpuContext;
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::grid::Grid2D;
use henad_core::helpers::f32_param;
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::provenance::ModelCode;
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{EdgeView, GridView, PointView, StatDescriptor, StatEntry, StatValue};

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

/// Waits for a build that runs, steps the loaded model once and takes its snapshots until one is past `tick`, or
/// panics after ten seconds.
pub fn step_past(app: &mut AppState, tick: u64) {
    app.wait_for_build();
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

/// Returns the text that `read` extracts from each accessible node that `add` draws in one frame.
pub fn accessible_texts(
    add: impl FnMut(&mut egui::Ui),
    read: impl Fn(&egui::accesskit::Node) -> Option<&str>,
) -> Vec<String> {
    let context = egui::Context::default();
    context.enable_accesskit();
    let output = context.run_ui(egui::RawInput::default(), add);
    let texts = output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| read(node).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    output.drop_without_applying_deltas();
    texts
}

/// Returns the accessible names of the widgets `add` draws in one frame.
pub fn accessible_names(add: impl FnMut(&mut egui::Ui)) -> Vec<String> {
    accessible_texts(add, |node| node.label())
}

/// Gate that holds the threads waiting on it until a test opens it.
#[derive(Clone, Default)]
pub struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    /// Blocks until the gate is open.
    pub fn wait(&self) {
        let (open, opened) = &*self.0;
        let mut open = open.lock().expect("no thread panics holding the gate");
        while !*open {
            open = opened.wait(open).expect("no thread panics holding the gate");
        }
    }

    /// Opens the gate, and releases every thread waiting on it.
    pub fn open(&self) {
        let (open, opened) = &*self.0;
        // A test that panics opens the gate on its way out, and a thread may have panicked holding it then.
        *open.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        opened.notify_all();
    }
}

/// Guard that opens a [`Gate`] when dropped, so a test that fails never leaves threads waiting on it.
pub struct OpenOnDrop(pub Gate);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// Way the first run of a [`held`] entry holds each of its steps until a gate opens.
#[derive(Clone)]
pub enum StepHold {
    /// The step waits on the gate on the thread it runs on.
    Step(Gate),
    /// The step occupies every worker of the pool it runs in, each waiting on the gate.
    Pool(Gate),
}

/// Record of what the builds and the states of a [`held`] entry did, each event under the number of its build from 1.
#[derive(Default)]
pub struct Probe {
    /// Number of builds started.
    pub builds: AtomicUsize,
    /// Number of each build that has returned.
    pub built: Mutex<Vec<usize>>,
    /// Name of the thread each build ran on.
    pub build_threads: Mutex<Vec<(usize, Option<String>)>>,
    /// Number of states dropped when each build started.
    pub drops_seen: Mutex<Vec<(usize, usize)>>,
    /// Name of the thread each step ran on.
    pub step_threads: Mutex<Vec<(usize, Option<String>)>>,
    /// Index of each action run, with the tick it ran at.
    pub actions: Mutex<Vec<(usize, (usize, u64))>>,
    /// Index of each parameter set, with its value.
    pub params: Mutex<Vec<(usize, (usize, ParamValue))>>,
    /// Number of states dropped. A state is dropped on its runner's thread as the thread ends.
    pub dropped: AtomicUsize,
    /// Number of workers a [`StepHold::Pool`] step holds.
    pub held_workers: AtomicUsize,
}

impl Probe {
    /// Returns the events of `events` that build `build` recorded.
    pub fn of<T: Clone>(events: &Mutex<Vec<(usize, T)>>, build: usize) -> Vec<T> {
        events
            .lock()
            .expect("no thread panics holding the probe")
            .iter()
            .filter(|(number, _)| *number == build)
            .map(|(_, event)| event.clone())
            .collect()
    }
}

/// Returns the name of the calling thread.
fn thread_name() -> Option<String> {
    std::thread::current().name().map(str::to_owned)
}

/// Returns `entry`, a CPU entry, with its builds and states recorded in `probe`.
///
/// The first build waits on `build_gate` before it builds, and each step of its state is held as `step_hold` says.
/// Every later build and state runs free.
pub fn held(
    entry: &ModelEntry,
    probe: &Arc<Probe>,
    build_gate: Option<Gate>,
    step_hold: Option<StepHold>,
) -> ModelEntry {
    let probe = Arc::clone(probe);
    entry.clone().wrap_factory(move |inner| {
        Arc::new(
            move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| -> Result<ModelState, Fault> {
                let build = probe.builds.fetch_add(1, Ordering::SeqCst) + 1;
                probe
                    .build_threads
                    .lock()
                    .expect("no thread panics holding the probe")
                    .push((build, thread_name()));
                probe
                    .drops_seen
                    .lock()
                    .expect("no thread panics holding the probe")
                    .push((build, probe.dropped.load(Ordering::SeqCst)));
                if build == 1
                    && let Some(gate) = &build_gate
                {
                    gate.wait();
                }
                let state = match inner(params, seed, gpu)? {
                    ModelState::Cpu(state) => ModelState::Cpu(Box::new(Held {
                        inner: state,
                        build,
                        probe: Arc::clone(&probe),
                        step_hold: step_hold.clone().filter(|_| build == 1),
                    })),
                    gpu @ ModelState::Gpu(_) => gpu,
                };
                probe
                    .built
                    .lock()
                    .expect("no thread panics holding the probe")
                    .push(build);
                Ok(state)
            },
        )
    })
}

/// CPU state that records its steps, actions and parameter edits in a [`Probe`], and holds its steps on a gate.
struct Held {
    inner: Box<dyn SimState>,
    /// Number of the build that built the state.
    build: usize,
    probe: Arc<Probe>,
    step_hold: Option<StepHold>,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.probe.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl SimState for Held {
    fn step(&mut self) {
        self.probe
            .step_threads
            .lock()
            .expect("no thread panics holding the probe")
            .push((self.build, thread_name()));
        match &self.step_hold {
            Some(StepHold::Step(gate)) => gate.wait(),
            Some(StepHold::Pool(gate)) => {
                use rayon::prelude::*;
                let probe = &self.probe;
                // One item per worker, each its own job, so every worker takes one and waits.
                (0..rayon::current_num_threads())
                    .into_par_iter()
                    .with_max_len(1)
                    .for_each(|_| {
                        probe.held_workers.fetch_add(1, Ordering::SeqCst);
                        gate.wait();
                    });
            }
            None => {}
        }
        self.inner.step();
    }
    fn tick(&self) -> u64 {
        self.inner.tick()
    }
    fn grid_view(&self) -> Option<GridView<'_>> {
        self.inner.grid_view()
    }
    fn point_view(&self) -> Option<PointView<'_>> {
        self.inner.point_view()
    }
    fn edge_view(&self) -> Option<EdgeView<'_>> {
        self.inner.edge_view()
    }
    fn prepare_view(&mut self) {
        self.inner.prepare_view();
    }
    fn stats(&self) -> Vec<StatEntry> {
        self.inner.stats()
    }
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.probe
            .params
            .lock()
            .expect("no thread panics holding the probe")
            .push((self.build, (index, value.clone())));
        self.inner.set_param(index, value)
    }
    fn act(&mut self, index: usize) -> bool {
        self.probe
            .actions
            .lock()
            .expect("no thread panics holding the probe")
            .push((self.build, (index, self.inner.tick())));
        self.inner.act(index)
    }
    fn set_layout(&mut self, on: bool, budget_ms: f32) -> bool {
        self.inner.set_layout(on, budget_ms)
    }
    fn relax_layout(&mut self) {
        self.inner.relax_layout();
    }
    fn population(&self) -> u64 {
        self.inner.population()
    }
    fn heap_bytes(&self) -> usize {
        self.inner.heap_bytes()
    }
    fn parallel_jobs(&self) -> Option<usize> {
        self.inner.parallel_jobs()
    }
}
