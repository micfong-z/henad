//! Models and helpers the app's tests share: SIR, replaced model code, a model with a time scale, a finished run and a
//! scratch folder, held builds and steps, the widgets of a frame, a harness that draws the sheet editor and reads its
//! accessible nodes, a harness that draws the graph canvas alone, and the alignment of a sheet rebuilt in the editor
//! with the starter it rebuilds.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use henad_compute::entry::{ModelEntry, ModelSet, ModelState, register_grid_model};
use henad_compute::fault::Fault;
use henad_compute::gpu::GpuContext;
use henad_compute::snapshot::Snapshot;
use henad_core::action::{ActionDescriptor, Schedule};
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::explore::measure::SeriesBuffer;
use henad_core::explore::outcome::{PlannedRun, RunOutcome, RunStatus, StopReason};
use henad_core::explore::replay::Replay;
use henad_core::grid::Grid2D;
use henad_core::helpers::f32_param;
use henad_core::metadata::Backend;
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_core::provenance::ModelCode;
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{EdgeView, GridView, PointView, StatDescriptor, StatEntry, StatValue};
use henad_explore::model_file::ModelFile;
use henad_graph::GraphFile;
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::value::{InputValue, PortRef};

use henad_graph::sheet::document::{CardRuns, SheetDocument, SheetProperty};
use henad_graph::sheet::sentence::is_forbidden_word;
use henad_graph::sheet::starters;
use web_time::Instant;

use crate::state::AppState;
use crate::ui::about::about_modal;
use crate::ui::editor::graph::canvas::pin_key;
use crate::ui::editor::graph::{GraphEditor, GraphRequest, graph_ui};
use crate::ui::editor::preview::{DEBOUNCE, poll_at};
use crate::ui::editor::sheet::new_model::{NewModelHeading, new_model_modal};
use crate::ui::editor::{editor_ui, open_text, replace_question_modal, unsaved_modal};
use crate::ui::menu_bar::file_menu;

/// Returns an app over the example models on a headless device that runs compute shaders, or `None` to skip on a
/// machine without a device.
pub fn headless_app() -> Option<AppState> {
    AppState::headless(henad_models::example_models(), true)
}

/// Text of the chance SIR, a copy of the sheet henad-graph's and henad-models' tests read.
pub const SIR_CHANCE: &str = include_str!("sheets/sir_chance.toml");

/// Returns the text of the sheet starter whose file is named `file_name`.
pub fn starter_text(file_name: &str) -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .unwrap_or_else(|| panic!("no sheet starter is named {file_name}"))
        .text
}

/// Returns `text` with its first `old` replaced by `new`.
///
/// # Panics
///
/// Panics if `text` does not hold `old`. Otherwise a test whose text moved would run on the unedited text.
pub fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds {old:?}");
    text.replacen(old, new, 1)
}

/// Returns Game of Life with the old state of line 2 left blank. Line 2 is then skipped.
pub fn life_with_a_skipped_line() -> String {
    edited(
        starter_text("game_of_life.toml"),
        "new_state = \"alive\"\nstart_state = \"dead\"\n",
        "new_state = \"alive\"\n",
    )
}

/// Returns the entry of SIR from the example models.
pub fn sir() -> ModelEntry {
    henad_models::example_models()
        .get("sir")
        .cloned()
        .expect("SIR is registered")
}

/// Returns the outcome of `run` with `run_key`, a run that stepped to its end: no ticks, no reducers and no series,
/// with a population of 1 and a millisecond to build and to run.
///
/// A test sets the fields it reads with struct update syntax.
pub fn finished_run(run: PlannedRun, run_key: u64) -> RunOutcome {
    RunOutcome {
        run,
        run_key,
        status: RunStatus::Ok,
        stop_reason: StopReason::Steps,
        ticks: 0,
        population: 1,
        build_ms: 1.0,
        wall_ms: 1.0,
        reducers: Vec::new(),
        series: SeriesBuffer::new(0),
        note: None,
    }
}

/// Empty folder in the system's temporary folder, removed with everything in it when dropped.
pub struct ScratchFolder(pub PathBuf);

impl ScratchFolder {
    /// Returns an empty folder named `henad-app-{name}-{pid}`, removing what an earlier run left there.
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("henad-app-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("a scratch folder");
        Self(path)
    }
}

impl Drop for ScratchFolder {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

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

/// Returns the entry of henad-models' fixture `sir_rates.toml`, SIR with rates on a GPU grid, whose hidden
/// `tick_length` parameter holds the length of one tick in days.
pub fn sir_rates() -> ModelEntry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../henad-models/tests/fixtures/lowered/rates/sir_rates.toml");
    let text = std::fs::read_to_string(&path).expect("the fixture reads");
    ModelFile::parse(&text)
        .expect("the fixture reads as a model file")
        .entry()
        .expect("the fixture builds an entry")
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

/// Sets the selection's grid width and height to `side` cells.
pub fn set_grid_side(app: &mut AppState, side: u32) {
    for id in ["grid_width", "grid_height"] {
        let index = app
            .selected_entry()
            .and_then(|entry| entry.param_index(id))
            .expect("a grid model declares its size");
        app.param_values[index] = ParamValue::U32(side);
    }
}

/// Returns `entry` under a factory that first creates a buffer past the device's limit, labelled
/// `henad_app_test_oversized`. The buffer is created on the thread the build runs on, outside the entry's own error
/// scopes.
pub fn with_oversized_buffer(entry: ModelEntry) -> ModelEntry {
    entry.wrap_factory(|inner| {
        Arc::new(
            move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| -> Result<ModelState, Fault> {
                if let Some(ctx) = gpu {
                    drop(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("henad_app_test_oversized"),
                        size: ctx.device.limits().max_buffer_size + 4096,
                        usage: wgpu::BufferUsages::STORAGE,
                        mapped_at_creation: false,
                    }));
                }
                inner(params, seed, gpu)
            },
        )
    })
}

/// Returns an app over the example GPU models whose adapter, as the app sees it, cannot run compute shaders, with no
/// model offered and none selected, or `None` to skip on a machine without a device.
pub fn gpu_only_app() -> Option<AppState> {
    let mut models = ModelSet::new(henad_core::build_info!());
    for entry in &henad_models::example_models() {
        if entry.metadata().backend == Backend::Gpu {
            models.insert(entry.clone()).expect("example ids are unique");
        }
    }
    assert!(!models.is_empty(), "the example models include GPU models");
    AppState::headless(models, false)
}

/// Returns a replay of ten ticks of model `model` at `params`, seed 1, with no scheduled actions.
pub fn replay(model: &str, params: Vec<ParamValue>) -> Replay {
    Replay {
        model: model.to_owned(),
        params,
        seed: 1,
        schedule: Schedule::default(),
        ticks: 10,
        label: "Sweep run 0".to_owned(),
    }
}

/// Returns `rect`, bounds in AccessKit's coordinates, as egui's rect.
pub fn bounds(rect: egui::accesskit::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(rect.x0 as f32, rect.y0 as f32),
        egui::pos2(rect.x1 as f32, rect.y1 as f32),
    )
}

/// Returns the events of the three frames that click the primary button at `position`: the move there, the press and
/// the release.
pub fn click_events(position: egui::Pos2) -> [Vec<egui::Event>; 3] {
    let button = |pressed| egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    [
        vec![egui::Event::PointerMoved(position)],
        vec![button(true)],
        vec![button(false)],
    ]
}

/// Polls the Editor's preview once now and once after the debounce, as two frames that far apart would.
pub fn settle_preview(app: &mut AppState) {
    let context = egui::Context::default();
    let now = Instant::now();
    poll_at(&context, app, now);
    poll_at(&context, app, now + DEBOUNCE);
}

/// Longest time [`wait_until`] waits for a thread to reach the state a test expects.
pub const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// Polls `condition` until it holds, or panics with `what` after [`DEADLINE`].
pub fn wait_until(what: &str, condition: impl FnMut() -> bool) {
    wait_within(what, DEADLINE, condition);
}

/// Polls `condition` until it holds, or panics with `what` after `limit`.
pub fn wait_within(what: &str, limit: std::time::Duration, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + limit;
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Returns the value of the stat `label` in `snapshot`.
pub fn stat(snapshot: &Snapshot, label: &str) -> f64 {
    snapshot
        .stats
        .iter()
        .find(|entry| entry.label() == label)
        .map(|entry| entry.value().scalar())
        .expect("the stat is published")
}

/// Returns the value of the stat `label` in the loaded run's snapshot at tick 0, or panics after [`DEADLINE`].
pub fn tick_zero_stat(app: &mut AppState, label: &str) -> f64 {
    app.wait_for_build();
    stat(wait_for_snapshot(app, |tick| tick == 0), label)
}

/// Takes the loaded run's snapshots until one whose tick passes `accept`, keeps it as the app's snapshot and returns
/// it, or panics after [`DEADLINE`].
pub fn wait_for_snapshot(app: &mut AppState, accept: impl Fn(u64) -> bool) -> &Snapshot {
    let thread = app.sim_thread.as_mut().expect("a model is loaded");
    let mut found = None;
    wait_until("a snapshot of the loaded model that passes the test", || {
        found = thread.take_snapshot().filter(|snapshot| accept(snapshot.tick));
        found.is_some()
    });
    app.snapshot.insert(found.expect("the snapshot that passed"))
}

/// Asserts that the loaded model built into a runner with no fault. The message names `what` and the fault.
pub fn assert_built(app: &AppState, what: &str) {
    assert!(
        app.sim_thread.is_some() && app.fault.is_none(),
        "{what} failed to build: {:?}",
        app.fault
    );
}

/// Waits for a build that runs, steps the loaded model once and takes its snapshots until one is past `tick`, or
/// panics after [`DEADLINE`].
pub fn step_past(app: &mut AppState, tick: u64) {
    app.wait_for_build();
    app.step_once();
    wait_for_snapshot(app, |seen| seen > tick);
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

// The sheet editor in a frame

/// Size in points of a screen that holds every section of a long sheet and every line of its card.
pub const TALL_SCREEN: egui::Vec2 = egui::vec2(1800.0, 2600.0);

/// An egui context that draws the File menu, the Editor tab, the New model dialog, the unsaved changes dialog and the
/// About modal over an app, driven by synthetic input.
pub struct Harness {
    /// Context the frames run in, with AccessKit on.
    pub context: egui::Context,
    /// App the frames draw.
    pub app: AppState,
    /// Time of the last frame in seconds.
    pub time: f64,
    /// Whether the window holds the system's keyboard focus, as an app driven through its inspection port may not.
    pub window_focused: bool,
    /// Size of the screen in points. A tall screen keeps every section of a long sheet in view.
    pub screen: egui::Vec2,
    /// Accessible name of the node that held the keyboard focus in the last frame, empty for none.
    pub focused: String,
}

/// Returns the top of `node` on the screen.
pub fn top(node: &Node) -> f32 {
    node.bounds.map_or(0.0, |rect| rect.top())
}

/// One accessible node of a frame.
#[derive(Debug, Clone)]
pub struct Node {
    /// Accessible name.
    pub label: String,
    /// Value, which holds a label's text.
    pub value: String,
    /// Accessible description.
    pub description: String,
    /// Role.
    pub role: egui::accesskit::Role,
    /// Bounds on the screen.
    pub bounds: Option<egui::Rect>,
    /// Whether the node is disabled.
    pub disabled: bool,
    /// Whether the node is a modal dialog, which makes the app behind it inert.
    pub modal: bool,
    /// Accessible name of the nearest dialog that holds the node, empty for a node in no dialog.
    pub dialog: String,
    /// Name of the nearest menu that holds the node, taken from the node that labels the menu, empty for a node in no
    /// menu.
    pub menu: String,
}

/// Returns the accessible nodes of `update`, each with the name of the nearest dialog that holds it.
fn nodes_of(update: &egui::accesskit::TreeUpdate) -> Vec<Node> {
    let parents: std::collections::HashMap<egui::accesskit::NodeId, egui::accesskit::NodeId> = update
        .nodes
        .iter()
        .flat_map(|(id, node)| node.children().iter().map(move |child| (*child, *id)))
        .collect();
    let dialogs: std::collections::HashMap<egui::accesskit::NodeId, &str> = update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Dialog)
        .map(|(id, node)| (*id, node.label().unwrap_or_default()))
        .collect();
    let dialog_of = |id: egui::accesskit::NodeId| {
        std::iter::successors(Some(id), |id| parents.get(id).copied())
            .find_map(|ancestor| dialogs.get(&ancestor))
            .map_or_else(String::new, |label| (*label).to_owned())
    };
    let texts: std::collections::HashMap<egui::accesskit::NodeId, &str> = update
        .nodes
        .iter()
        .map(|(id, node)| (*id, node.label().or(node.value()).unwrap_or_default()))
        .collect();
    let menus: std::collections::HashMap<egui::accesskit::NodeId, &str> = update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Menu)
        .map(|(id, node)| {
            let name = node.label().unwrap_or_else(|| {
                node.labelled_by()
                    .first()
                    .and_then(|label| texts.get(label).copied())
                    .unwrap_or_default()
            });
            (*id, name)
        })
        .collect();
    let menu_of = |id: egui::accesskit::NodeId| {
        std::iter::successors(parents.get(&id).copied(), |id| parents.get(id).copied())
            .find_map(|ancestor| menus.get(&ancestor))
            .map_or_else(String::new, |name| (*name).to_owned())
    };
    update
        .nodes
        .iter()
        .map(|(id, node)| Node {
            dialog: dialog_of(*id),
            menu: menu_of(*id),
            // A submenu's button ends with its arrow.
            label: node.label().unwrap_or_default().trim_end_matches(" ⏵").to_owned(),
            value: node.value().unwrap_or_default().to_owned(),
            description: node.description().unwrap_or_default().to_owned(),
            role: node.role(),
            disabled: node.is_disabled(),
            modal: node.is_modal(),
            bounds: node.bounds().map(bounds),
        })
        .collect()
}

impl Harness {
    /// Returns a harness over the example models, or `None` to skip on a machine without a device.
    pub fn new() -> Option<Self> {
        let harness = Self {
            context: egui::Context::default(),
            app: headless_app()?,
            time: 0.0,
            window_focused: true,
            screen: egui::vec2(1600.0, 1000.0),
            focused: String::new(),
        };
        harness.context.enable_accesskit();
        crate::ui::focus::install(&harness.context);
        Some(harness)
    }

    /// Returns a harness over the example models on a [`TALL_SCREEN`], or `None` to skip on a machine without a device.
    pub fn tall() -> Option<Self> {
        let mut harness = Self::new()?;
        harness.screen = TALL_SCREEN;
        Some(harness)
    }

    /// Runs one frame with `events`, and returns its accessible nodes.
    pub fn frame(&mut self, events: Vec<egui::Event>) -> Vec<Node> {
        self.time += 0.016;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.screen)),
            time: Some(self.time),
            events,
            focused: self.window_focused,
            ..egui::RawInput::default()
        };
        let app = &mut self.app;
        let mut output = self.context.run_ui(input, |ui| {
            egui::MenuBar::new().ui(ui, |ui| file_menu(ui, app));
            editor_ui(ui, app);
            new_model_modal(ui.ctx(), app);
            unsaved_modal(ui.ctx(), app);
            replace_question_modal(ui.ctx(), app);
            about_modal(ui.ctx(), app);
        });
        output.textures_delta.clear();
        let update = output.platform_output.accesskit_update.expect("an AccessKit update");
        self.focused = update
            .nodes
            .iter()
            .find(|(id, _)| *id == update.focus)
            .and_then(|(_, node)| node.label())
            .unwrap_or_default()
            .to_owned();
        nodes_of(&update)
    }

    /// Runs a few frames with no input, and returns the accessible nodes of the last.
    pub fn settle(&mut self) -> Vec<Node> {
        for _ in 0..3 {
            self.frame(Vec::new());
        }
        self.frame(Vec::new())
    }

    /// Clicks the Name field of the New model dialog.
    pub fn click_name_field(&mut self) {
        let nodes = self.settle();
        let field = nodes
            .iter()
            .find(|node| node.role == egui::accesskit::Role::TextInput)
            .and_then(|node| node.bounds)
            .expect("the Name field")
            .center();
        self.click_at(field);
    }

    /// Clicks the primary button at `position`.
    pub fn click_at(&mut self, position: egui::Pos2) {
        for events in click_events(position) {
            self.frame(events);
        }
    }

    /// Returns the centre of the one node whose label ends with `label`.
    pub fn find(&mut self, label: &str) -> egui::Pos2 {
        let nodes = self.settle();
        let found: Vec<&Node> = nodes
            .iter()
            .filter(|node| node.label.ends_with(label) && node.bounds.is_some())
            .collect();
        assert_eq!(found.len(), 1, "one widget reads {label}: {found:?}");
        found[0].bounds.expect("bounds").center()
    }

    /// Clicks the one widget whose label ends with `label`.
    pub fn click(&mut self, label: &str) {
        let position = self.find(label);
        self.click_at(position);
    }

    /// Presses `key` with `modifiers`.
    pub fn press(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![event(true), event(false)]);
    }

    /// Replaces the text of the field that holds the keyboard focus with `text`, through select all and a typed text.
    pub fn replace_text(&mut self, text: &str) {
        self.press(egui::Key::A, egui::Modifiers::COMMAND);
        self.frame(vec![egui::Event::Text(text.to_owned())]);
    }

    /// Returns the label and the value of every node of a settled frame. A label's text is its node's value.
    pub fn labels(&mut self) -> Vec<String> {
        self.settle()
            .into_iter()
            .flat_map(|node| [node.label, node.value])
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Returns the label, the value and the description of every node of a settled frame.
    pub fn accessible_texts(&mut self) -> Vec<String> {
        texts_of(self.settle())
    }

    /// Returns the label, the value and the description of every node of a settled frame that lies outside the
    /// dialog named `dialog`.
    pub fn accessible_texts_outside(&mut self, dialog: &str) -> Vec<String> {
        texts_of(self.settle().into_iter().filter(|node| node.dialog != dialog))
    }

    /// Clicks the button named `label`, exactly, inside the dialog named `dialog`.
    pub fn click_in(&mut self, dialog: &str, label: &str) {
        let nodes = self.settle();
        let found: Vec<&Node> = nodes
            .iter()
            .filter(|node| node.dialog == dialog && node.label == label && node.bounds.is_some())
            .collect();
        assert_eq!(found.len(), 1, "one {label} in {dialog}: {found:?}");
        let position = found[0].bounds.expect("bounds").center();
        self.click_at(position);
        self.settle();
    }

    /// Clicks the heading named `title` in the open New model dialog, and runs frames until the headings it opens and
    /// closes have finished their animation.
    pub fn click_heading(&mut self, title: &str) {
        self.click_in(henad_graph::sheet::sentence::NEW_MODEL, title);
        for _ in 0..3 {
            self.settle();
        }
    }

    /// Opens Advanced and `heading` under it in the open New model dialog, then presses Create, which opens the
    /// starter chosen under `heading`: the grid rule, or the first graph starter.
    pub fn create_under(&mut self, heading: NewModelHeading) {
        use henad_graph::sheet::sentence::{CREATE, NEW_MODEL};
        self.click_heading(NewModelHeading::Advanced.title());
        self.click_heading(heading.title());
        self.click_in(NEW_MODEL, CREATE);
    }

    /// Opens the sheet `text` in the Editor and draws it.
    pub fn open_sheet(&mut self, text: &str) {
        open_text(&mut self.app.editor, text.to_owned(), Some("opened.toml".to_owned()));
        self.settle();
    }
}

/// Returns the label, the value and the description of each of `nodes` that is not empty.
fn texts_of(nodes: impl IntoIterator<Item = Node>) -> Vec<String> {
    nodes
        .into_iter()
        .flat_map(|node| [node.label, node.value, node.description])
        .filter(|text| !text.is_empty())
        .collect()
}

/// Returns the words of `text`, split at every character that is not a letter or a digit.
pub fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
}

/// Returns whether `text` is the whole name of a control that opens or creates a node graph, which the words rule
/// exempts: the New model dialog's heading Node graph, Convert to node graph, Edit as node graph and the sheet's line
/// that opens its copy.
///
/// An icon before the name is left out of the match. The conversion dialog is exempt by place instead, through
/// [`Harness::accessible_texts_outside`].
pub fn opens_a_node_graph(text: &str) -> bool {
    use henad_graph::sheet::sentence::{CONVERT_HEADING, EDIT_AS_NODE_GRAPH, NODE_GRAPH_HEADING, converted_link};

    let name = text.trim_start_matches(|c: char| !c.is_alphanumeric());
    [NODE_GRAPH_HEADING, CONVERT_HEADING, EDIT_AS_NODE_GRAPH].contains(&name)
        || name.starts_with(converted_link("").as_str())
}

/// Returns every property of `document`: those of the cells, of each kind of agent, then of the ground.
fn all_properties(document: &SheetDocument) -> impl Iterator<Item = &SheetProperty> {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
}

/// Returns every forbidden word in `texts`, with the text that holds it.
pub fn forbidden_words(texts: &[String]) -> Vec<(String, String)> {
    texts
        .iter()
        .filter(|text| !opens_a_node_graph(text))
        .flat_map(|text| {
            words(text)
                .filter(|word| is_forbidden_word(word))
                .map(|word| (word.to_owned(), text.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

// The graph canvas in a frame

/// Frames a [`CanvasHarness`] runs after a change, long enough for egui's sizing passes to settle.
const SETTLE_FRAMES: usize = 4;

/// Returns node `number`'s id.
pub fn n(number: u32) -> NodeId {
    NodeId::new(number).expect("an id")
}

/// Returns the link to the output `port` of node `node`.
pub fn link(node: NodeId, port: &str) -> InputValue {
    InputValue::Link(PortRef {
        node,
        port: Some(port.to_owned()),
    })
}

/// Returns an editor of the graph starter `name`.
pub fn graph_editor_of(name: &str) -> GraphEditor {
    let text = henad_graph::starters::starter(name).expect("a starter").text;
    GraphEditor::new(&GraphFile::parse(text).expect("the starter reads"))
}

/// Returns the edit commands that add the parameter `spread_rate` and bind the infection term of the SIR starter's
/// step, input `b` of node 12, to it.
pub fn bind_spread_rate() -> Vec<EditCommand> {
    vec![
        EditCommand::AddParam(ParamDescriptor::new(
            "spread_rate",
            "Spread rate",
            ParamKind::F32 {
                min: 0.0,
                max: 1.0,
                default: 0.4,
                step: None,
            },
        )),
        EditCommand::BindParam {
            scope: Scope::pass("step"),
            node: n(12),
            port: "b".to_owned(),
            param: "spread_rate".to_owned(),
        },
    ]
}

/// An egui context that draws a [`GraphEditor`] alone, driven by synthetic input.
pub struct CanvasHarness {
    /// Context the frames run in.
    pub context: egui::Context,
    /// Graph the frames draw.
    pub graph: GraphEditor,
    /// Time of the last frame in seconds.
    time: f64,
    /// Modifiers held in the coming frames.
    pub modifiers: egui::Modifiers,
}

impl CanvasHarness {
    /// Returns a harness over the starter `name`, its tick pass open and arranged, and every node in view.
    pub fn new(name: &str) -> Self {
        let mut harness = Self::over(graph_editor_of(name));
        harness.settle();
        // The starters place some nodes edge to edge, where a pin of one node covers a pin of the next.
        harness.graph.request(GraphRequest::Arrange);
        harness.settle();
        harness.settle_framing();
        harness
    }

    /// Returns a harness over `graph` as it is, before any frame runs.
    pub fn over(graph: GraphEditor) -> Self {
        Self {
            context: egui::Context::default(),
            graph,
            time: 0.0,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Runs frames until the framing of the open canvas has settled, or panics after twenty.
    pub fn settle_framing(&mut self) {
        for _ in 0..20 {
            let scope = self.scope();
            if self.graph.canvas(&scope).is_some_and(|canvas| canvas.framing.is_none()) {
                return;
            }
            self.frame(Vec::new());
        }
        panic!("the framing never settled");
    }

    /// Returns the nodes selected on the open canvas.
    pub fn selected(&self) -> Vec<NodeId> {
        self.graph
            .canvas(&self.scope())
            .expect("a canvas")
            .selected(&self.context)
    }

    /// Runs one frame with `events`, and returns its output.
    pub fn frame_output(&mut self, mut events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 0.016;
        events.insert(0, egui::Event::ModifiersChanged(self.modifiers));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            time: Some(self.time),
            events,
            ..egui::RawInput::default()
        };
        let graph = &mut self.graph;
        let mut output = self.context.run_ui(input, |ui| graph_ui(ui, graph));
        output.textures_delta.clear();
        output
    }

    /// Runs one frame with `events`.
    pub fn frame(&mut self, events: Vec<egui::Event>) {
        drop(self.frame_output(events));
    }

    /// Runs frames with no input.
    pub fn settle(&mut self) {
        for _ in 0..SETTLE_FRAMES {
            self.frame(Vec::new());
        }
    }

    pub fn scope(&self) -> Scope {
        self.graph.scope().clone()
    }

    /// Returns the screen position of a canvas position.
    pub fn screen(&self, canvas: egui::Pos2) -> egui::Pos2 {
        self.graph.canvas(&self.scope()).expect("a canvas").transform * canvas
    }

    /// Returns the screen position of the top of node `id`, a point on its header that holds no widget.
    pub fn node_grip(&self, id: NodeId) -> egui::Pos2 {
        let rect = self
            .graph
            .canvas(&self.scope())
            .and_then(|canvas| canvas.current_rect(id))
            .expect("the node is drawn");
        self.screen(rect.center_top() + egui::vec2(0.0, 4.0))
    }

    /// Returns the screen position of the centre of a pin of node `id`.
    pub fn pin(&self, id: NodeId, output: bool, index: usize) -> egui::Pos2 {
        let canvas = self.graph.canvas(&self.scope()).expect("a canvas");
        let key = pin_key(canvas.canvas_id, id, output, index);
        let rect = self
            .context
            .data(|data| data.get_temp::<egui::Rect>(key))
            .expect("the pin is drawn");
        self.screen(rect.center())
    }

    /// Returns the index of the input `port` of node `id` on the canvas.
    pub fn input_index(&self, id: NodeId, port: &str) -> usize {
        let canvas = self.graph.canvas(&self.scope()).expect("a canvas");
        let view = canvas.snarl.get_node(canvas.nodes[&id]).expect("a view");
        view.inputs
            .iter()
            .position(|input| input.port == port)
            .expect("the port")
    }

    pub fn button(&self, position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: self.modifiers,
        }
    }

    /// Drags the primary button from `start` to `end` in screen points.
    pub fn drag(&mut self, start: egui::Pos2, end: egui::Pos2) {
        self.frame(vec![egui::Event::PointerMoved(start)]);
        self.frame(vec![self.button(start, true)]);
        for step in 1..=6 {
            let reached = start + (end - start) * (step as f32 / 6.0);
            self.frame(vec![egui::Event::PointerMoved(reached)]);
        }
        self.frame(vec![self.button(end, false)]);
        self.settle();
    }

    /// Presses the primary button on `pin` and moves far enough to start a wire, and leaves the button down.
    pub fn start_wire(&mut self, pin: egui::Pos2) {
        self.frame(vec![egui::Event::PointerMoved(pin)]);
        self.frame(vec![self.button(pin, true)]);
        for step in 1..=3 {
            self.frame(vec![egui::Event::PointerMoved(
                pin + egui::vec2(0.0, step as f32 * 10.0),
            )]);
        }
    }

    /// Clicks the primary button with no modifier.
    pub fn click(&mut self, position: egui::Pos2) {
        self.frame(vec![egui::Event::PointerMoved(position)]);
        self.frame(vec![self.button(position, true)]);
        self.frame(vec![self.button(position, false)]);
        self.settle();
    }

    /// Presses `key` with `modifiers`.
    pub fn press(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![event(true), event(false)]);
        self.settle();
    }

    /// Returns a screen position on the canvas background, in its bottom right corner.
    pub fn background(&self) -> egui::Pos2 {
        self.graph
            .canvas(&self.scope())
            .expect("a canvas")
            .canvas_rect
            .right_bottom()
            - egui::vec2(12.0, 12.0)
    }

    /// Returns the value of the input `port` of node `id` in the document.
    pub fn input(&self, id: NodeId, port: &str) -> Option<InputValue> {
        let pass = self.graph.open_pass().expect("a pass");
        let node = pass.body.node(id)?;
        node.inputs.get(port).cloned()
    }

    /// Returns whether the open body holds node `id`.
    pub fn holds(&self, id: NodeId) -> bool {
        self.graph.open_pass().expect("a pass").body.node(id).is_some()
    }

    /// Clicks the secondary button at `position`, which opens a menu of the canvas.
    pub fn secondary_click(&mut self, position: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![egui::Event::PointerMoved(position)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.settle();
    }

    /// Returns the accessible nodes of a frame, after a frame that turns AccessKit on.
    pub fn accessible_nodes(&mut self) -> Vec<Node> {
        self.context.enable_accesskit();
        self.frame(Vec::new());
        let output = self.frame_output(Vec::new());
        nodes_of(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("an AccessKit update"),
        )
    }

    /// Returns the screen rectangle of the one widget whose accessible label or value is `label`.
    pub fn accessible(&mut self, label: &str) -> egui::Rect {
        self.accessible_with_role(label, None)
    }

    /// Returns the screen rectangle of the one widget of role `role`, when set, whose accessible label or value is
    /// `label`.
    pub fn accessible_with_role(&mut self, label: &str, role: Option<egui::accesskit::Role>) -> egui::Rect {
        let found: Vec<egui::Rect> = self
            .accessible_nodes()
            .into_iter()
            .filter(|node| node.label == label || node.value == label)
            .filter(|node| role.is_none_or(|role| node.role == role))
            .filter_map(|node| node.bounds)
            .collect();
        assert_eq!(found.len(), 1, "one widget reads {label}: {found:?}");
        found[0]
    }

    /// Runs `request` now and lets the canvas follow.
    pub fn run(&mut self, request: GraphRequest) {
        self.graph.request(request);
        self.settle();
    }
}

// Rebuilt starters

/// Returns `starter` with the parts the sheet editor does not build yet taken out: every card line, every card that
/// runs each tick, and the number of the next line.
///
/// The editor adds a card that runs each tick with its first line. The number of the next line returns to 1, as in a
/// blank model.
pub fn without_card_lines(starter: &SheetDocument) -> SheetDocument {
    let mut stripped = starter.clone();
    stripped.cards.retain(|card| card.runs != CardRuns::EachTick);
    for card in &mut stripped.cards {
        card.lines.clear();
        card.line_tables.clear();
    }
    stripped.sheet.next_line = 1;
    stripped
}

/// Returns `rebuilt`, a sheet built in the editor, aligned with `starter`, the starter it rebuilds, on what says
/// nothing about the editor.
///
/// Each drawing line takes the key of the starter's line of the same id, and each start the key of the starter's
/// property of the same id, when both draw, whatever kind of thing holds the property. A parameter takes the step, the
/// meaning and the format of the starter's parameter of the same id. No control sets any of them. A state or a measure
/// takes the starter's colour when the palette names the two colours alike, since a starter's colour can lie outside
/// the palette the editor offers. No id is copied.
pub fn align_with_starter(rebuilt: &SheetDocument, starter: &SheetDocument) -> SheetDocument {
    use crate::ui::editor::sheet::widgets::color_name;

    let mut aligned = rebuilt.clone();
    for card in &mut aligned.cards {
        for (id, line) in &mut card.line_tables {
            if line.key.is_some()
                && let Some((_, theirs)) = starter.line(id)
                && theirs.key.is_some()
            {
                line.key = theirs.key;
            }
        }
    }
    let their_properties: Vec<&SheetProperty> = all_properties(starter).collect();
    let agents = aligned.agents.iter_mut().flat_map(|kind| &mut kind.properties);
    let ground = aligned.ground.iter_mut().flat_map(|ground| &mut ground.properties);
    for property in aligned.cells.iter_mut().flatten().chain(agents).chain(ground) {
        let Some(theirs) = their_properties.iter().find(|known| known.id == property.id) else {
            continue;
        };
        if let (Some(start), Some(their_start)) = (&mut property.starts_as, &theirs.starts_as)
            && start.key.is_some()
            && their_start.key.is_some()
        {
            start.key = their_start.key;
        }
        for state in &mut property.states {
            if let Some(their_state) = theirs.states.iter().find(|known| known.id == state.id)
                && color_name(state.color) == color_name(their_state.color)
            {
                state.color = their_state.color;
            }
        }
    }
    for parameter in &mut aligned.parameters {
        if let Some(theirs) = starter.parameters.iter().find(|known| known.id == parameter.id) {
            parameter.step = theirs.step;
            parameter.means.clone_from(&theirs.means);
            parameter.format = theirs.format;
        }
    }
    for measure in &mut aligned.measures {
        if let Some(theirs) = starter.measures.iter().find(|known| known.id == measure.id)
            && color_name(measure.color) == color_name(theirs.color)
        {
            measure.color = theirs.color;
        }
    }
    aligned
}
