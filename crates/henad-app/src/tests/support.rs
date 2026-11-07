//! Models and helpers the app's tests share: replaced model code, a model with a time scale, held builds and steps,
//! the widgets of a frame, a harness that draws the sheet editor and reads its accessible nodes, and the alignment of
//! a sheet rebuilt in the editor with the starter it rebuilds.

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
use henad_explore::model_file::ModelFile;

use henad_graph::sheet::document::{CardRuns, SheetDocument, SheetProperty};
use henad_graph::sheet::sentence::FORBIDDEN_WORDS;

use crate::state::AppState;
use crate::ui::about::about_modal;
use crate::ui::editor::sheet::new_model::new_model_modal;
use crate::ui::editor::{editor_ui, open_text};
use crate::ui::menu_bar::file_menu;

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

/// Returns the value of the stat `label` in the loaded run's snapshot at tick 0, or panics after ten seconds.
pub fn tick_zero_stat(app: &mut AppState, label: &str) -> f64 {
    app.wait_for_build();
    let snapshot = wait_for_snapshot(app, |tick| tick == 0);
    snapshot
        .stats
        .iter()
        .find(|entry| entry.label() == label)
        .map(|entry| entry.value().scalar())
        .expect("the stat is published")
}

/// Takes the loaded run's snapshots until one whose tick passes `accept`, keeps it as the app's snapshot and returns
/// it, or panics after ten seconds.
pub fn wait_for_snapshot(app: &mut AppState, accept: impl Fn(u64) -> bool) -> &henad_compute::snapshot::Snapshot {
    let thread = app.sim_thread.as_mut().expect("a model is loaded");
    for _ in 0..1000 {
        if let Some(snapshot) = thread.take_snapshot()
            && accept(snapshot.tick)
        {
            return app.snapshot.insert(snapshot);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("no snapshot of the loaded model passed the test");
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

// The sheet editor in a frame

/// An egui context that draws the File menu, the Editor tab, the New model dialog and the About modal over an app,
/// driven by synthetic input.
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
}

impl Harness {
    /// Returns a harness over the example models, or `None` to skip on a machine without a device.
    pub fn new() -> Option<Self> {
        let harness = Self {
            context: egui::Context::default(),
            app: AppState::headless(henad_models::example_models(), true)?,
            time: 0.0,
            window_focused: true,
            screen: egui::vec2(1600.0, 1000.0),
            focused: String::new(),
        };
        harness.context.enable_accesskit();
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
        update
            .nodes
            .iter()
            .map(|(_, node)| Node {
                // A submenu's button ends with its arrow.
                label: node.label().unwrap_or_default().trim_end_matches(" ⏵").to_owned(),
                value: node.value().unwrap_or_default().to_owned(),
                description: node.description().unwrap_or_default().to_owned(),
                role: node.role(),
                bounds: node.bounds().map(|rect| {
                    egui::Rect::from_min_max(
                        egui::pos2(rect.x0 as f32, rect.y0 as f32),
                        egui::pos2(rect.x1 as f32, rect.y1 as f32),
                    )
                }),
            })
            .collect()
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
        let button = |pressed| egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![egui::Event::PointerMoved(position)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
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

    /// Returns the label and the value of every node of a settled frame. A label's text is its node's value.
    pub fn labels(&mut self) -> Vec<String> {
        self.settle()
            .into_iter()
            .flat_map(|node| [node.label, node.value])
            .filter(|text| !text.is_empty())
            .collect()
    }
}

impl Harness {
    /// Returns the label, the value and the description of every node of a settled frame.
    pub fn accessible_texts(&mut self) -> Vec<String> {
        self.settle()
            .into_iter()
            .flat_map(|node| [node.label, node.value, node.description])
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Opens the sheet `text` in the Editor and draws it.
    pub fn open_sheet(&mut self, text: &str) {
        open_text(&mut self.app.editor, text.to_owned(), Some("opened.toml".to_owned()));
        self.settle();
    }
}

/// Returns the words of `text`, split at every character that is not a letter or a digit.
pub fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
}

/// Returns whether `text` belongs to a control that opens or creates a node graph, which the words rule exempts.
pub fn opens_a_node_graph(text: &str) -> bool {
    text.contains(crate::ui::editor::NEW_NODE_GRAPH)
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
                .filter(|word| {
                    FORBIDDEN_WORDS
                        .iter()
                        .any(|forbidden| forbidden.eq_ignore_ascii_case(word))
                })
                .map(|word| (word.to_owned(), text.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
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
