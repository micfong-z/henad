//! State of the app's panels. `DockArea::show_inside` borrows it as its `TabViewer` beside the dock.

use std::sync::Arc;

use egui::TextureHandle;
use henad_compute::cpu::sim_thread::{SimCommand, WakeFn};
use henad_compute::entry::{ModelEntry, ModelLookupError, ModelSet};
use henad_compute::fault::{BUILDING, Fault, FaultSink};
use henad_compute::simulation::{RunSetup, SetupError};
use henad_compute::snapshot::Snapshot;
use henad_core::action::Schedule;
use henad_core::explore::replay::Replay;
use henad_core::metadata::Backend;
use henad_core::params::ParamValue;
use henad_core::provenance::ModelSource;
use henad_core::view::StatsHistory;
use henad_explore::output::memory::SweepFiles;

use crate::options::{AppOpening, Product};
use crate::sim_runner::SimRunner;
use crate::ui::agent_layer::AgentLayer;
use crate::ui::dock::Tab;
use crate::ui::edge_layer::EdgeStyle;
use crate::ui::export::Recording;
use crate::ui::export::image::PendingCapture;
use crate::ui::files::open::spawn_open;
use crate::ui::files::save::{spawn_save, spawn_save_files};
use crate::ui::files::{OpenOutcome, OpenTarget, SaveOutcome, SaveResult, SaveTarget};
use crate::ui::params::{INVALID_SEED, parse_seed};
use crate::ui::results::ResultsPanel;
use crate::ui::sweep::SweepPanel;
use henad_compute::runtime_info::RuntimeInfo;

use henad_compute::gpu::GpuContext;
use henad_compute::gpu::timing::{DEFAULT_BATCH_SIZE, DEFAULT_TARGET_MS};

mod build;
mod install;
mod pool;

pub use build::{BUILDING_MODEL, DID_NOT_STOP};
use build::{PendingBuild, Stopping};
#[cfg(test)]
pub use build::{STILL_STOPPING, STOP_WAIT};
pub use install::{PendingRebuild, remap_schedule, remap_values};
pub use pool::ModelPool;

/// Exponential moving average smoothing factor (0..1, higher = more responsive).
const EMA_ALPHA: f64 = 0.1;

/// Reason shown for the disabled Build button when no model is selected.
pub const NO_SELECTED_MODEL: &str = "Select a model in the Model tab";

/// Default number of snapshots the chart history keeps before it starts dropping the oldest snapshots.
pub const DEFAULT_HISTORY_LEN: usize = 10_000;

/// Default time in milliseconds that a snapshot can spend on a network's layout.
const DEFAULT_LAYOUT_BUDGET_MS: f32 = 4.0;

/// Status shown while a save is pending, followed by the file name.
#[cfg(not(target_arch = "wasm32"))]
const SAVE_PENDING: &str = "Select location to save";

/// Status shown while a download is pending, followed by the file name.
#[cfg(target_arch = "wasm32")]
const SAVE_PENDING: &str = "Downloading";

/// Status of the downloads of several files. A browser can hold back every download after the first.
const DOWNLOADS_STARTED: &str = "Downloads started. Press Save results again if your browser blocked any.";

/// Per-frame timing breakdown, smoothed with EMA.
#[derive(Default)]
pub struct FrameTimings {
    pub render_ms: f64,
    pub ui_ms: f64,
    /// Raw viewport cost this frame, folded into the EMAs once the frame is over.
    pub frame_render_ms: f64,
}

impl FrameTimings {
    pub fn update_ema(smoothed: &mut f64, sample_ms: f64) {
        *smoothed += EMA_ALPHA * (sample_ms - *smoothed);
    }
}

pub struct AppState {
    /// Kept so a freshly built sim thread can be given a repaint waker.
    egui_ctx: egui::Context,
    /// Every model the host offers, including GPU models this machine cannot run.
    pub models: ModelSet,
    /// Name, build, links and command line of the app the host ships.
    pub product: Product,
    /// Opening that the app could not open, shown in the Model panel until a model is selected.
    pub opening_refusal: Option<OpeningRefusal>,
    /// Id of the model the panels show, `None` when no offered model runs on this machine or the opening was rejected.
    pub selected_model: Option<String>,
    pub param_values: Vec<ParamValue>,
    /// Entry the live simulation was built from. It stays the running model's entry after the set's entry under its
    /// id is replaced, until a build replaces the simulation.
    loaded: Option<ModelEntry>,
    pub pending_reload: Vec<bool>,
    /// Seed of the next build, `None` for the model's default seed.
    pub seed: Option<u64>,
    /// Text of the Seed field, including text that does not parse.
    pub seed_text: String,
    /// Values the loaded model runs with, those it was built with and any live edit since, in the order of the loaded
    /// entry's parameters. Meaningful only while a model is loaded.
    pub loaded_values: Vec<ParamValue>,
    /// Seed the loaded model was built with. Meaningful only while a model is loaded.
    pub loaded_seed: Option<u64>,
    /// Actions the next build runs at their ticks.
    pub schedule: Schedule,
    /// Schedule the loaded model was built with.
    pub loaded_schedule: Schedule,
    // Entry row of the scheduled-actions list.
    pub schedule_action_input: usize,
    pub schedule_tick_input: u64,
    /// Sweep run the loaded model replays.
    pub opened_run: Option<OpenedRun>,
    /// Rebuild of the loaded model under replaced code, waiting on the Rebuild model modal.
    pub pending_rebuild: Option<PendingRebuild>,
    /// Tick a pending [`SimCommand::RunTo`] stops at.
    pub run_to_target: Option<u64>,
    pub run_to_input: u64,
    /// Tab brought to the front once the dock has drawn.
    pub focus_request: Option<Tab>,
    /// Runner of the live simulation. `None` while a build runs, until [`Self::poll_build`] takes the build's runner.
    pub sim_thread: Option<SimRunner>,
    /// Build of the live simulation on its own thread. Always `None` in a browser, where builds run inline.
    building: Option<PendingBuild>,
    /// Runs and builds that did not stop in time, kept until their threads end. A build started meanwhile holds them
    /// too, and waits for them.
    stopping: Vec<Arc<Stopping>>,
    /// Pool the app's CPU work runs in. A fresh pool replaces it once a detached thread has held it past the timeout.
    pools: ModelPool,
    /// Generation of the pool the live runner runs in.
    runner_generation: u64,
    /// Sink the live CPU runner stores a panic out of its loop in. A teardown replaces it, so a detached runner's late
    /// fault reaches no later run.
    pub sim_faults: FaultSink,
    pub snapshot: Option<Snapshot>,
    pub sim_running: bool,
    pub grid_texture: Option<TextureHandle>,
    /// Separate from `grid_texture`, so density mode does not overwrite a composite model's field.
    pub density_texture: Option<TextureHandle>,
    pub density_max: f32,
    pub point_render_mode: PointRenderMode,
    // Edge toggles from the Viewport toolbar, for network models.
    pub show_edges: bool,
    pub edge_arrows: bool,
    /// Agent renderer, built on first use and kept across model switches. Its pipeline is tied to no model.
    pub agent_layer: Option<AgentLayer>,
    /// Serial of the snapshot whose data the viewport last copied to the GPU.
    pub last_rendered_serial: Option<u64>,
    pub rendering_enabled: bool,
    pub target_tps: f64,
    pub uncapped: bool,
    pub ticks_per_snapshot: u32,
    // Spring layout settings for network models, sent to each model when it is built.
    pub layout_on: bool,
    pub layout_budget_ms: f32,
    pub layout_while_paused: bool,
    pub stats_history: Option<StatsHistory>,
    /// Number of samples the chart history keeps, `None` to keep every sample so a whole run can be exported.
    pub history_capacity: Option<usize>,
    /// Position of the History length slider, kept while Unlimited is ticked so unticking restores it.
    pub history_len: usize,
    /// Host, adapter and device limits, collected once at startup.
    pub runtime: RuntimeInfo,
    /// Device and queue for rendering, present wherever the app runs. The renderer and a live GPU
    /// simulation report their errors into `faults`, and a live CPU simulation into `sim_faults`. A GPU
    /// sweep runs on its own device, and its errors stay with the sweep. `gpu_ctx` below is a different
    /// thing, and gates GPU models.
    pub render_ctx: GpuContext,
    /// The fault being shown, cleared when the user dismisses the modal.
    pub fault: Option<ShownFault>,
    pub about_open: bool,
    /// Note shown in the Performance tab when the browser's thread pool failed to start.
    pub thread_pool_note: Option<String>,
    /// About window's image, set when the window first opens. It holds `None` for an icon that is not a PNG.
    pub logo_texture: std::cell::OnceCell<Option<TextureHandle>>,
    pub timings: FrameTimings,
    /// Device and queue a live GPU model builds on, `None` where the adapter cannot run compute shaders. The GPU
    /// models are then hidden.
    pub gpu_ctx: Option<GpuContext>,
    /// A viewport capture waiting on the GPU.
    pub capture: Option<PendingCapture>,
    pub recording: Recording,
    /// The last export's result, shown in the Export tab.
    pub export_status: Option<String>,
    /// Channel the save dialogs send their outcomes back on, from their own thread or task.
    saves: (flume::Sender<SaveOutcome>, flume::Receiver<SaveOutcome>),
    /// Channel the open dialogs send their outcomes back on, from their own thread or task.
    opens: (flume::Sender<OpenOutcome>, flume::Receiver<OpenOutcome>),
    pub sweep: SweepPanel,
    pub results: ResultsPanel,
    // GPU batching controls
    pub gpu_adaptive: bool,
    pub gpu_target_ms: f64,
    pub gpu_batch_size: u32,
}

/// Fault the modal shows, with the model it stopped.
#[derive(Debug)]
pub struct ShownFault {
    pub fault: Fault,
    /// Name of the model the fault stopped, `None` when no model was building or loaded.
    pub model: Option<String>,
}

/// Opening that the app could not open, as shown in the Model panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningRefusal {
    /// Line naming what was not opened, as in "Run not opened".
    pub lead: &'static str,
    /// Reason, as in "This build does not include model 'x'".
    pub reason: String,
}

/// Tick an opened run or setup is stepped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenAt {
    /// Tick 0, paused.
    Start,
    /// A tick reached by stepping uncapped from tick 0.
    Tick(u64),
}

/// Sweep run the loaded model was built from.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedRun {
    pub replay: Replay,
    /// Source of the entry that built the run when it was opened.
    pub source: ModelSource,
    /// Whether a live edit, an action, or a build from other values or other code has left the run's trajectory.
    pub modified: bool,
}

impl OpenedRun {
    /// Returns whether a build from `params`, `seed` and `schedule` steps through the run tick for tick.
    pub fn is_built_by(&self, params: &[ParamValue], seed: Option<u64>, schedule: &Schedule) -> bool {
        params == self.replay.params.as_slice() && seed == Some(self.replay.seed) && *schedule == self.replay.schedule
    }
}

/// Draw style for an agent population in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointRenderMode {
    #[default]
    Agents,
    /// Density map of the agents. Cheaper past roughly a million agents, and readable where sprites would overlap
    /// into a mass.
    Density,
}

impl AppState {
    /// Returns the state of an app over `models`, with the first model that runs on this machine selected, or
    /// with nothing selected when no model runs on this machine.
    ///
    /// `gpu_ctx` is `None` on an adapter without compute. GPU models are then hidden from the Model panel, and
    /// rendering is unaffected.
    pub fn new(
        egui_ctx: egui::Context,
        models: ModelSet,
        product: Product,
        render_ctx: GpuContext,
        gpu_ctx: Option<GpuContext>,
        runtime: RuntimeInfo,
    ) -> Self {
        let first = models.runnable(gpu_ctx.as_ref()).next();
        let selected_model = first.map(|entry| entry.id().to_owned());
        let param_values: Vec<ParamValue> = first.map(default_values).unwrap_or_default();
        let pools = ModelPool::default();
        let mut sweep = SweepPanel::default();
        sweep.cli_command.clone_from(&product.cli_command);
        sweep.model_pool = pools.clone();

        Self {
            egui_ctx,
            pending_reload: vec![false; param_values.len()],
            models,
            product,
            opening_refusal: None,
            selected_model,
            param_values,
            loaded: None,
            seed: None,
            seed_text: String::new(),
            loaded_values: Vec::new(),
            loaded_seed: None,
            schedule: Schedule::default(),
            loaded_schedule: Schedule::default(),
            schedule_action_input: 0,
            schedule_tick_input: 0,
            opened_run: None,
            pending_rebuild: None,
            run_to_target: None,
            run_to_input: 0,
            focus_request: None,
            sim_thread: None,
            building: None,
            stopping: Vec::new(),
            pools,
            runner_generation: 0,
            sim_faults: FaultSink::new(),
            snapshot: None,
            sim_running: false,
            grid_texture: None,
            density_texture: None,
            density_max: 4.0,
            point_render_mode: PointRenderMode::default(),
            show_edges: true,
            edge_arrows: false,
            agent_layer: None,
            last_rendered_serial: None,
            rendering_enabled: true,
            target_tps: 30.0,
            uncapped: false,
            ticks_per_snapshot: 1,
            layout_on: true,
            layout_budget_ms: DEFAULT_LAYOUT_BUDGET_MS,
            layout_while_paused: false,
            stats_history: None,
            history_capacity: Some(DEFAULT_HISTORY_LEN),
            history_len: DEFAULT_HISTORY_LEN,
            runtime,
            render_ctx,
            fault: None,
            about_open: false,
            thread_pool_note: None,
            logo_texture: std::cell::OnceCell::new(),
            timings: FrameTimings::default(),
            gpu_ctx,
            capture: None,
            recording: Recording::Off,
            export_status: None,
            saves: flume::unbounded(),
            opens: flume::unbounded(),
            sweep,
            results: ResultsPanel::default(),
            gpu_adaptive: true,
            gpu_target_ms: DEFAULT_TARGET_MS,
            gpu_batch_size: DEFAULT_BATCH_SIZE,
        }
    }

    /// Tears down the live simulation and builds the selected model from the fields the next build reads.
    ///
    /// Does nothing when [`Self::build_setup`] rejects the fields. Build is disabled then, with the reason
    /// [`setup_message`] returns.
    pub fn reset_simulation(&mut self) {
        let setup = match self.build_setup() {
            Some(Ok(setup)) => Some(setup),
            Some(Err(error)) => {
                log::warn!("Build refused: {}", setup_message(&error));
                return;
            }
            None => None,
        };
        self.load(setup);
    }

    /// Tears down the live simulation and builds `setup`. The setup's entry then becomes the loaded entry, and `None`
    /// tears down alone.
    ///
    /// The loaded fields are the setup's from the start of the build, and commands sent while it runs wait for its
    /// runner. A build that fails goes to the fault modal and leaves no model loaded.
    fn load(&mut self, setup: Option<RunSetup>) {
        self.settle_opened_run(setup.as_ref());
        self.stop_recording();
        // A runner that stops releases a GPU model's buffers and pipelines, and the build waits for one that has not
        // stopped yet. A paint callback still in flight this frame holds its own `Arc` to the display, and keeps the
        // texture alive.
        self.stop_simulation();
        drop(self.render_ctx.faults.take());
        self.snapshot = None;
        self.last_rendered_serial = None;
        self.grid_texture = None;
        self.density_texture = None;
        self.density_max = 4.0;
        self.ticks_per_snapshot = 1;
        self.loaded = None;
        self.pending_rebuild = None;
        self.export_status = None;
        self.run_to_target = None;
        // The next model may have no agents at all.
        if let Some(layer) = &mut self.agent_layer {
            layer.clear();
        }

        let Some(setup) = setup else {
            return;
        };

        self.stats_history = Some(StatsHistory::new(
            setup.entry().stat_descriptors().to_vec(),
            self.history_capacity,
        ));
        self.sim_running = false;
        // The fields of the next build are the setup's only for the selected model. A rebuild of a model loaded
        // while another one is selected leaves them alone.
        if self.selected_model.as_deref() == Some(setup.entry().id()) {
            self.pending_reload = vec![false; self.param_values.len()];
        }
        self.loaded_values = setup.values().to_vec();
        self.loaded_seed = setup.seed();
        self.loaded_schedule = setup.schedule().clone();
        self.loaded = Some(setup.entry().clone());

        let schedule = setup.schedule().clone();
        self.start_build(setup);
        if !schedule.is_empty() {
            self.send_command(SimCommand::SetSchedule(schedule));
        }
    }

    /// Clears the opened run before a build of another model or a teardown, or sets its `modified` to whether
    /// `setup` differs from the run in its values or its code.
    ///
    /// A rebuild from the run's own values and code clears the mark that [`Self::mark_opened_run_modified`] sets.
    fn settle_opened_run(&mut self, setup: Option<&RunSetup>) {
        let Some(run) = &mut self.opened_run else {
            return;
        };
        let Some(setup) = setup.filter(|setup| setup.entry().id() == run.replay.model) else {
            self.opened_run = None;
            return;
        };
        run.modified =
            run.source != *setup.entry().source() || !run.is_built_by(setup.values(), setup.seed(), setup.schedule());
    }

    /// Opens `opening`, the results, run or setup that the host requested.
    ///
    /// A run or a setup this machine cannot open leaves the app with nothing selected, and the Model panel shows the
    /// reason.
    pub fn open(&mut self, opening: AppOpening) {
        let opened = match opening {
            #[cfg(not(target_arch = "wasm32"))]
            AppOpening::Results(folder) => {
                crate::ui::results::open_folder(self, folder);
                self.focus_request = Some(Tab::Results);
                Ok(())
            }
            AppOpening::Run { replay, open_at } => self.open_run(replay, open_at).map_err(|reason| OpeningRefusal {
                lead: "Run not opened",
                reason,
            }),
            AppOpening::Setup { setup, open_at } => self.open_setup(&setup, open_at).map_err(|reason| OpeningRefusal {
                lead: "Model not opened",
                reason,
            }),
        };
        if let Err(refusal) = opened {
            log::warn!("{}: {}", refusal.lead, refusal.reason);
            self.selected_model = None;
            self.param_values.clear();
            self.pending_reload.clear();
            self.schedule = Schedule::default();
            self.opening_refusal = Some(refusal);
            self.focus_request = Some(Tab::Model);
        }
    }

    /// Builds the run `replay` holds, optionally steps it to a tick, and brings the viewport to the front.
    ///
    /// # Errors
    ///
    /// Returns a message when this build or this machine has no model `replay.model`, or the model declares a different
    /// number of parameters. A build that fails goes to the fault modal instead, and leaves no run open.
    pub fn open_run(&mut self, replay: Replay, start: OpenAt) -> Result<(), String> {
        let declared = self
            .lookup(&replay.model)
            .map_err(|error| lookup_message(&error))?
            .param_descriptors()
            .len();
        if replay.params.len() != declared {
            return Err(format!(
                "This run sets {} {}, but model '{}' has {declared}",
                replay.params.len(),
                crate::ui::plural(replay.params.len() as u64, "parameter"),
                replay.model
            ));
        }

        let entry = self.lookup(&replay.model).map_err(|error| lookup_message(&error))?;
        RunSetup::from_replay(entry, &replay).map_err(|error| setup_message(&error))?;

        let built = self.open_values(
            &replay.model,
            &replay.params,
            Some(replay.seed),
            &replay.schedule,
            start,
        );
        if built && let Some(loaded) = &self.loaded {
            self.opened_run = Some(OpenedRun {
                replay,
                source: loaded.source().clone(),
                modified: false,
            });
        }
        Ok(())
    }

    /// Builds the model of the set under `setup`'s model id from the setup's values, seed and schedule, optionally
    /// steps it to a tick, and brings the viewport to the front.
    ///
    /// The setup's fields are stored in the fields the next build reads, and a default seed stays the default.
    ///
    /// # Errors
    ///
    /// Returns a message when this build or this machine has no model under the setup's id, or that model rejects the
    /// setup's values. A build that fails goes to the fault modal instead.
    pub fn open_setup(&mut self, setup: &RunSetup, start: OpenAt) -> Result<(), String> {
        let id = setup.entry().id();
        let entry = self.lookup(id).map_err(|error| lookup_message(&error))?;
        RunSetup::from_parts(entry, setup.values(), setup.seed(), setup.schedule().clone())
            .map_err(|error| setup_message(&error))?;
        self.open_values(id, setup.values(), setup.seed(), setup.schedule(), start);
        Ok(())
    }

    /// Selects model `id`, builds it from `params`, `seed` and `schedule`, steps it to `start`, and brings the
    /// viewport to the front.
    ///
    /// Returns whether the build runs or has succeeded. A run to a tick sent while the build runs waits for its runner.
    fn open_values(
        &mut self,
        id: &str,
        params: &[ParamValue],
        seed: Option<u64>,
        schedule: &Schedule,
        start: OpenAt,
    ) -> bool {
        self.opened_run = None;
        self.opening_refusal = None;
        self.selected_model = Some(id.to_owned());
        self.param_values = params.to_vec();
        self.pending_reload = vec![false; params.len()];
        self.seed = seed;
        self.seed_text = seed.map(|seed| seed.to_string()).unwrap_or_default();
        self.schedule = schedule.clone();
        self.schedule_action_input = 0;
        self.reset_simulation();
        if !self.has_simulation() {
            return false;
        }
        if let OpenAt::Tick(tick) = start {
            self.run_to_input = tick;
            self.run_to(tick);
        }
        self.focus_request = Some(Tab::Viewport);
        true
    }

    /// Steps the loaded model uncapped to `tick` and pauses there.
    ///
    /// A tick behind the current one rebuilds first. Note that the rebuild builds the selected model, which is the
    /// loaded one only while [`Self::selection_is_loaded`] holds.
    pub fn run_to(&mut self, tick: u64) {
        if self.snapshot.as_ref().is_some_and(|snap| tick < snap.tick) {
            self.reset_simulation();
        }
        if !self.has_simulation() {
            return;
        }
        self.send_command(SimCommand::RunTo(tick));
        self.sim_running = false;
        self.run_to_target = Some(tick);
    }

    /// Marks the opened run modified, after a live edit or an action.
    pub fn mark_opened_run_modified(&mut self) {
        if let Some(run) = &mut self.opened_run {
            run.modified = true;
        }
    }

    /// Returns whether the Seed field holds a seed the loaded model was not built with.
    pub fn seed_pending(&self) -> bool {
        self.selection_is_loaded() && self.loaded_seed != self.seed
    }

    /// Returns whether the scheduled actions differ from those the loaded model was built with.
    pub fn schedule_pending(&self) -> bool {
        self.selection_is_loaded() && self.loaded_schedule != self.schedule
    }

    /// Returns a waker that repaints the UI.
    ///
    /// A sim thread or a sweep calls it after publishing, so an idle UI picks the result up next frame instead of
    /// waiting for whatever input event happens to arrive.
    #[cfg_attr(
        all(target_arch = "wasm32", target_feature = "atomics"),
        expect(
            clippy::arc_with_non_send_sync,
            reason = "a `WakeFn` is not `Send` on wasm with atomics"
        )
    )]
    pub fn repaint_waker(&self) -> WakeFn {
        let ctx = self.egui_ctx.clone();
        Arc::new(move || ctx.request_repaint())
    }

    /// Pauses the live simulation, a run to a tick included.
    pub fn pause_simulation(&mut self) {
        if !self.has_simulation() {
            return;
        }
        if self.sim_running || self.run_to_target.is_some() {
            self.send_command(SimCommand::Pause);
        }
        self.sim_running = false;
        self.run_to_target = None;
    }

    /// Offloads the live simulation and passes the fault to the modal. A running sweep carries on.
    ///
    /// A fault while building belongs to the selected model, and any other to the loaded one. The Model panel stays
    /// usable while a model runs, and the selection can be another model by then.
    pub fn report_fault(&mut self, fault: Fault) {
        let model = if fault.during == BUILDING {
            self.selected_entry()
        } else {
            self.loaded_entry()
        };
        let model = model.map(|entry| entry.name().to_owned());
        self.show_fault(fault, model);
    }

    /// Offloads the live simulation and passes the fault to the modal, naming `model` as the model it stopped.
    fn show_fault(&mut self, fault: Fault, model: Option<String>) {
        log::error!("{fault}");
        self.offload_simulation();
        self.fault = Some(ShownFault { fault, model });
    }

    /// Tears down the live simulation, or abandons the build that runs, and leaves no model loaded.
    ///
    /// Neither waits past [`STOP_WAIT`]. A runner or a build still running then is detached.
    pub fn offload_simulation(&mut self) {
        self.stop_recording();
        self.stop_simulation();
        drop(self.render_ctx.faults.take());
        self.snapshot = None;
        self.sim_running = false;
        self.grid_texture = None;
        self.density_texture = None;
        self.last_rendered_serial = None;
        self.stats_history = None;
        self.loaded = None;
        self.opened_run = None;
        self.pending_rebuild = None;
        self.run_to_target = None;
        if let Some(layer) = &mut self.agent_layer {
            layer.clear();
        }
    }

    pub fn layout_command(&self) -> SimCommand {
        SimCommand::SetLayout {
            on: self.layout_on,
            budget_ms: self.layout_budget_ms,
            while_paused: self.layout_while_paused,
        }
    }

    pub fn edge_style(&self) -> EdgeStyle {
        EdgeStyle {
            visible: self.show_edges,
            arrows: self.edge_arrows,
        }
    }

    /// Returns whether the live simulation runs the selected model under the set's entry for it, the same id with the
    /// same source.
    ///
    /// Note that a model whose code the set has replaced since its build is not the selection, and live edits and
    /// actions do not reach it.
    pub fn selection_is_loaded(&self) -> bool {
        self.loaded.as_ref().is_some_and(|loaded| {
            self.selected_entry()
                .is_some_and(|selected| selected.id() == loaded.id() && selected.source() == loaded.source())
        })
    }

    /// Returns whether the live simulation runs the selected model under code that the set has replaced since its
    /// build.
    pub fn code_pending(&self) -> bool {
        self.loaded.as_ref().is_some_and(|loaded| {
            self.selected_entry()
                .is_some_and(|selected| selected.id() == loaded.id() && selected.source() != loaded.source())
        })
    }

    /// Entry of the selected model.
    pub fn selected_entry(&self) -> Option<&ModelEntry> {
        self.models.get(self.selected_model.as_deref()?)
    }

    /// Returns the setup the next build reads, checked against the selected model, or `None` with no model selected.
    ///
    /// The setup holds `param_values`, `seed` and `schedule`, each checked as [`RunSetup::from_parts`] checks them.
    pub fn build_setup(&self) -> Option<Result<RunSetup, SetupError>> {
        let entry = self.selected_entry()?;
        Some(RunSetup::from_parts(
            entry,
            &self.param_values,
            self.seed,
            self.schedule.clone(),
        ))
    }

    /// Entry the live simulation was built from.
    ///
    /// Note that it can differ from the set's entry under its id, once [`Self::install_models`] has replaced that
    /// entry.
    pub fn loaded_entry(&self) -> Option<&ModelEntry> {
        self.loaded.as_ref()
    }

    /// Id of the model the live simulation was built from.
    pub fn loaded_id(&self) -> Option<&str> {
        self.loaded.as_ref().map(ModelEntry::id)
    }

    /// Returns the models of the set that run on this machine, in the set's order.
    pub fn offered_models(&self) -> impl Iterator<Item = &ModelEntry> + '_ {
        self.models.runnable(self.gpu_ctx.as_ref())
    }

    /// Returns entry `id`, or the reason this build or this machine cannot run it.
    ///
    /// # Errors
    ///
    /// Returns [`ModelLookupError::NotInSet`] for a model the host does not offer, and
    /// [`ModelLookupError::NeedsGpu`] for a GPU model on an adapter without compute.
    pub fn lookup(&self, id: &str) -> Result<&ModelEntry, ModelLookupError> {
        self.models.lookup(id, self.gpu_ctx.as_ref())
    }

    /// Selects model `id` with its default values and no scheduled actions, or the values, seed and actions the
    /// loaded model runs with when `id` is the loaded model.
    ///
    /// An already selected model keeps its values. An id missing from the set leaves no values. The values and actions
    /// of a loaded model whose code the set has replaced are carried to the set's entry by id, as
    /// [`remap_values`] and [`remap_schedule`] carry them.
    pub fn select_model(&mut self, id: &str) {
        if self.selected_model.as_deref() == Some(id) {
            return;
        }
        self.selected_model = Some(id.to_owned());
        self.opening_refusal = None;
        self.schedule_action_input = 0;
        if let Some(loaded) = self.loaded.as_ref().filter(|loaded| loaded.id() == id) {
            let current = self.models.get(id).unwrap_or(loaded);
            self.param_values = remap_values(
                loaded.param_descriptors(),
                &self.loaded_values,
                current.param_descriptors(),
            )
            .values;
            self.pending_reload = vec![false; self.param_values.len()];
            self.seed = self.loaded_seed;
            self.seed_text = self.loaded_seed.map(|seed| seed.to_string()).unwrap_or_default();
            self.schedule = remap_schedule(&self.loaded_schedule, current.action_descriptors()).schedule;
            return;
        }
        self.param_values = self.models.get(id).map(default_values).unwrap_or_default();
        self.pending_reload = vec![false; self.param_values.len()];
        // Entries index the previous model's actions.
        self.schedule = Schedule::default();
    }

    /// Sends a live edit of parameter `index` to the loaded model, and records `value` as a value that the model
    /// runs with.
    ///
    /// Returns whether the edit was sent. It is sent only while the selection is the loaded model.
    pub fn send_live_param(&mut self, index: usize, value: ParamValue) -> bool {
        if !self.selection_is_loaded() || !self.has_simulation() {
            return false;
        }
        self.send_command(SimCommand::SetParam {
            index,
            value: value.clone(),
        });
        if let Some(slot) = self.loaded_values.get_mut(index) {
            *slot = value;
        }
        true
    }

    /// Reasons this machine cannot build the selection. Always empty for a CPU model.
    pub fn selection_shortfalls(&self) -> Vec<String> {
        self.selected_entry().map_or_else(Vec::new, |entry| {
            entry.shortfalls(&self.param_values, &self.runtime.granted)
        })
    }

    /// Returns the reason Build cannot build the selection, or `None` when it can.
    ///
    /// Build is disabled with the reason as its tooltip. The reasons are no selection, a size the device cannot
    /// hold, a Seed field that does not parse and values that [`Self::build_setup`] rejects, in that order.
    pub fn build_refusal(&self) -> Option<String> {
        if self.selected_entry().is_none() {
            return Some(NO_SELECTED_MODEL.to_owned());
        }
        let shortfalls = self.selection_shortfalls();
        if !shortfalls.is_empty() {
            return Some(too_large_message(&shortfalls));
        }
        if parse_seed(&self.seed_text).is_err() {
            return Some(INVALID_SEED.to_owned());
        }
        self.build_setup()?.err().map(|error| setup_message(&error))
    }

    /// Returns whether the live simulation, or the model a build runs for, is a GPU model.
    pub fn is_gpu(&self) -> bool {
        match &self.sim_thread {
            Some(thread) => thread.gpu_stats().is_some(),
            None => {
                self.is_building()
                    && self
                        .loaded_entry()
                        .is_some_and(|entry| entry.metadata().backend == Backend::Gpu)
            }
        }
    }

    /// Opens a save dialog for `bytes` under the name `name`, for the panel `target`.
    ///
    /// Results can be polled via [`Self::poll_saves`].
    pub fn save_as(&mut self, target: SaveTarget, name: &str, bytes: Vec<u8>) {
        *self.save_status(target) = Some(format!("{SAVE_PENDING} {name}"));
        spawn_save(
            target,
            name.to_owned(),
            bytes,
            self.saves.0.clone(),
            self.egui_ctx.clone(),
        );
    }

    /// Opens a dialog that saves the four files of a sweep, `files`, together, for the panel `target`.
    ///
    /// Results can be polled via [`Self::poll_saves`].
    pub fn save_files(&mut self, target: SaveTarget, files: Arc<SweepFiles>) {
        *self.save_status(target) = Some(format!("{SAVE_PENDING} {} files", files.entries().len()));
        spawn_save_files(target, files, self.saves.0.clone(), self.egui_ctx.clone());
    }

    /// Returns the status line a save for `target` reports to.
    fn save_status(&mut self, target: SaveTarget) -> &mut Option<String> {
        match target {
            SaveTarget::Export => &mut self.export_status,
            SaveTarget::SweepSpec | SaveTarget::SweepResults(_) => &mut self.sweep.status,
        }
    }

    pub fn poll_saves(&mut self) {
        while let Ok(SaveOutcome { target, result }) = self.saves.1.try_recv() {
            // A download of one file counts as saved. The user requested it, and the app cannot read back anything
            // the browser asks the user. The downloads of several files leave them unsaved. A browser can hold back
            // every download after the first.
            if let SaveTarget::SweepResults(generation) = target
                && matches!(result, SaveResult::Saved(_) | SaveResult::Downloaded(_))
            {
                self.results.mark_files_saved(generation);
            }
            *self.save_status(target) = Some(match result {
                SaveResult::Saved(name) => format!("Saved {name}"),
                SaveResult::Downloaded(name) => format!("Downloaded {name}"),
                SaveResult::DownloadsStarted => DOWNLOADS_STARTED.to_owned(),
                SaveResult::Failed(message) => format!("Save failed: {message}"),
                SaveResult::Canceled => "Save canceled".to_owned(),
            });
        }
    }

    /// Opens a dialog that picks the files or the folder that `target` requests.
    ///
    /// Results can be polled via [`Self::poll_opens`].
    pub fn open_file(&self, target: OpenTarget) {
        spawn_open(target, self.opens.0.clone(), self.egui_ctx.clone());
    }

    pub fn poll_opens(&mut self) {
        while let Ok(OpenOutcome { target, result }) = self.opens.1.try_recv() {
            match target {
                OpenTarget::SweepSpec | OpenTarget::DesignTable | OpenTarget::OutputFolder => {
                    crate::ui::sweep::receive_open(self, target, result);
                }
                OpenTarget::Results => crate::ui::results::receive_open(self, result),
            }
        }
    }

    /// Appends the snapshot's stats to the recording, and stops the recording on a write error.
    pub fn record(&mut self, snapshot: &Snapshot) {
        if let Err(err) = self.recording.push(snapshot.tick, &snapshot.stats) {
            self.export_status = Some(format!("Recording stopped: {err}"));
            self.recording = Recording::Off;
        }
    }

    /// Stops recording and closes the CSV file.
    pub fn stop_recording(&mut self) {
        let to_tick = self.snapshot.as_ref().map_or(0, |snap| snap.tick);
        match std::mem::replace(&mut self.recording, Recording::Off).stop(to_tick) {
            Ok(recording) => self.recording = recording,
            Err(err) => self.export_status = Some(format!("Recording failed: {err}")),
        }
    }

    /// Draws the layers into an offscreen target at their own resolution and starts reading it back.
    ///
    /// Results can be polled via [`Self::poll_capture`].
    pub fn request_viewport_capture(&mut self, name: String) {
        match crate::ui::export::image::start(self, name) {
            Ok(pending) => {
                self.export_status = Some("Capturing viewport".to_owned());
                self.capture = Some(pending);
            }
            Err(err) => self.export_status = Some(format!("Capture failed: {err}")),
        }
    }

    pub fn poll_capture(&mut self) {
        let Some(pending) = &self.capture else {
            return;
        };
        let Some(result) = pending.poll(&self.render_ctx.device) else {
            return;
        };
        let name = pending.name.clone();
        self.capture = None;
        match result {
            Ok(png) => self.save_as(SaveTarget::Export, &name, png),
            Err(err) => self.export_status = Some(format!("Capture failed: {err}")),
        }
    }
}

#[cfg(test)]
impl AppState {
    /// Returns an app over `models` on a headless device, or `None` to skip on a machine without a device.
    ///
    /// Without `compute` the app sees an adapter that cannot run compute shaders, and hides its GPU models.
    ///
    /// # Panics
    ///
    /// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
    pub fn headless(models: ModelSet, compute: bool) -> Option<Self> {
        use henad_explore::testing::{TestDeviceRequest, headless_test_device};

        let ctx = headless_test_device(&TestDeviceRequest::raised(models.gpu_needs()))?;
        let runtime = ctx
            .runtime_info()
            .expect("a headless device carries its runtime info")
            .clone();
        let product = crate::options::AppOptions::new(
            ModelSet::new(henad_core::build_info!()),
            "Henad",
            henad_core::build_info!(),
        )
        .cli_command("henad-cli")
        .__official()
        .product;
        let gpu_ctx = compute.then(|| ctx.clone());
        Some(Self::new(
            egui::Context::default(),
            models,
            product,
            ctx,
            gpu_ctx,
            runtime,
        ))
    }
}

/// Returns the default value of every parameter of `entry`.
pub fn default_values(entry: &ModelEntry) -> Vec<ParamValue> {
    entry
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind().default_value())
        .collect()
}

/// Returns `error` capitalised, as in "This build does not include model 'x'".
pub fn lookup_message(error: &ModelLookupError) -> String {
    crate::ui::sweep::draft::capitalize(&error.to_string())
}

/// Returns `shortfalls`, the reasons the device cannot build a model, as Build's disabled reason.
pub fn too_large_message(shortfalls: &[String]) -> String {
    format!("Too large for this device: {}", shortfalls.join("; "))
}

/// Returns `error` as Build's disabled reason, as in `Parameter 'infection_rate': 2 is outside 0..=1`.
pub fn setup_message(error: &SetupError) -> String {
    use crate::ui::sweep::draft::describe_error;
    match error {
        SetupError::Param(reason) => describe_error(reason),
        other => describe_error(other),
    }
}

#[cfg(test)]
mod tests {
    use henad_core::action::{Schedule, Scheduled};
    use henad_core::explore::replay::Replay;
    use henad_core::params::ParamValue;

    use henad_compute::entry::ModelSet;
    use henad_compute::fault::{BUILDING, Fault, STEPPING};
    use henad_core::metadata::Backend;
    use henad_explore::output::details::{ChoiceForm, params_by_id_json};

    use super::{AppState, OpenAt, OpenedRun};
    use crate::options::AppOpening;
    use crate::ui::dock::Tab;
    use crate::ui::export::metadata::run_details;

    /// Returns an app over the example models whose adapter, as the app sees it, cannot run compute shaders, or `None`
    /// to skip on a machine without a device.
    fn app_without_compute() -> Option<AppState> {
        AppState::headless(henad_models::example_models(), false)
    }

    /// Returns a replay of model `model` with no parameters.
    fn replay_of(model: &str) -> Replay {
        Replay {
            model: model.to_owned(),
            params: Vec::new(),
            seed: 1,
            schedule: Schedule::default(),
            ticks: 10,
            label: "Sweep run 0".to_owned(),
        }
    }

    #[test]
    fn an_app_without_compute_hides_gpu_models_and_names_each_missing_one() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let offered: Vec<&str> = app.offered_models().map(|entry| entry.id()).collect();
        assert_eq!(
            offered,
            ["sir", "boids", "game_of_life", "ants", "virus_network", "team_assembly"]
        );
        assert_eq!(
            app.selected_model.as_deref(),
            Some("sir"),
            "the first model that runs here"
        );

        assert_eq!(
            app.open_run(replay_of("gpu_sir"), OpenAt::Start),
            Err("Model 'gpu_sir' needs a GPU with compute support, and this device has none".to_owned())
        );
        assert_eq!(
            app.open_run(replay_of("absent"), OpenAt::Start),
            Err("This build does not include model 'absent'".to_owned())
        );
        assert_eq!(
            app.selected_model.as_deref(),
            Some("sir"),
            "a refused run leaves the selection alone"
        );
    }

    #[test]
    fn a_build_follows_the_run_only_from_its_own_values() {
        let schedule = Schedule::from_entries(vec![Scheduled {
            index: 0,
            id: "seed_outbreak".to_owned(),
            tick: 50,
        }]);
        let params = vec![ParamValue::U32(64), ParamValue::F32(0.3)];
        let run = OpenedRun {
            source: henad_core::provenance::ModelSource::__from_type_path("henad_models::sir::SirGridModel"),
            replay: Replay {
                model: "sir".to_owned(),
                params: params.clone(),
                seed: 42,
                schedule: schedule.clone(),
                ticks: 300,
                label: "Sweep run 0: config 0, replicate 0".to_owned(),
            },
            modified: false,
        };

        assert!(run.is_built_by(&params, Some(42), &schedule));
        assert!(
            !run.is_built_by(&params, None, &schedule),
            "the Default seed matched seed 42"
        );
        assert!(!run.is_built_by(&params, Some(43), &schedule), "another seed");
        assert!(!run.is_built_by(&params, Some(42), &Schedule::default()), "no schedule");
        let edited = [ParamValue::U32(64), ParamValue::F32(0.4)];
        assert!(!run.is_built_by(&edited, Some(42), &schedule), "an edited parameter");
    }

    #[test]
    fn an_opened_setup_keeps_the_default_seed() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let sir = app.models.get("sir").expect("the example models include sir").clone();
        let setup = sir
            .setup()
            .set("infection_rate", 0.4_f32)
            .and_then(|setup| setup.act_at("seed_outbreak", 20))
            .expect("a valid setup");

        assert_eq!(app.open_setup(&setup, OpenAt::Start), Ok(()));
        app.wait_for_build();
        assert_eq!(app.selected_model.as_deref(), Some("sir"));
        assert_eq!(app.param_values, setup.values());
        assert_eq!(app.schedule, *setup.schedule());
        assert_eq!(app.seed, None, "the default seed became a number");
        assert_eq!(app.seed_text, "", "the Seed field shows the Default hint");
        assert!(app.sim_thread.is_some(), "the setup built");
        assert_eq!(app.loaded_seed, None);
        assert_eq!(app.loaded_schedule, *setup.schedule());
        assert_eq!(app.opened_run, None, "a setup is no sweep run");
        assert_eq!(app.focus_request, Some(Tab::Viewport));

        assert_eq!(app.open_setup(&setup.with_seed(7), OpenAt::Start), Ok(()));
        assert_eq!(app.seed, Some(7));
        assert_eq!(app.seed_text, "7");
        assert_eq!(app.loaded_seed, Some(7));
    }

    /// Picking the loaded model again restores its values. Otherwise the run details would record values the model
    /// was not built with.
    #[test]
    fn picking_the_loaded_model_again_restores_what_it_runs_with() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let sir = app.models.get("sir").expect("the example models include sir").clone();
        let setup = sir
            .setup()
            .set("infection_rate", 0.4_f32)
            .and_then(|setup| setup.act_at("seed_outbreak", 20))
            .expect("a valid setup")
            .with_seed(9);
        assert_eq!(app.open_setup(&setup, OpenAt::Start), Ok(()));
        app.wait_for_build();
        let details = |app: &AppState| -> serde_json::Value {
            serde_json::from_str(&run_details(app)).expect("the run details are JSON")
        };
        let recorded = params_by_id_json(sir.param_descriptors(), setup.values(), ChoiceForm::Name);

        app.select_model("boids");
        assert_eq!(
            details(&app)["params"],
            recorded,
            "another model's values under sir's ids"
        );
        app.select_model("sir");
        assert_eq!(app.param_values, setup.values());
        assert_eq!(app.pending_reload, vec![false; setup.values().len()]);
        assert_eq!((app.seed, app.seed_text.as_str()), (Some(9), "9"));
        assert_eq!(app.schedule, *setup.schedule());
        assert!(!app.seed_pending() && !app.schedule_pending());
        assert_eq!(details(&app)["params"], recorded);
        assert_eq!(details(&app)["params_match_running_model"], true);

        let (index, live) = sir
            .param_descriptors()
            .iter()
            .enumerate()
            .find(|(_, descriptor)| descriptor.is_live())
            .expect("sir has a live parameter");
        let edited = match live.kind().default_value() {
            ParamValue::F32(_) => ParamValue::F32(0.25),
            other => other,
        };
        app.param_values[index] = edited.clone();
        assert!(app.send_live_param(index, edited.clone()));
        app.select_model("boids");
        assert!(
            !app.send_live_param(index, edited.clone()),
            "an edit of a model not selected"
        );
        app.select_model("sir");
        assert_eq!(
            app.param_values[index], edited,
            "the live edit came back with the model"
        );
        assert_eq!(app.loaded_values[index], edited);
    }

    /// The modal shows the name of the model that faulted. The selection can be another model by then.
    #[test]
    fn a_fault_names_the_model_it_stopped() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let name = |app: &AppState, id: &str| app.models.get(id).map(|entry| entry.name().to_owned());
        app.reset_simulation();
        app.wait_for_build();
        assert_eq!(app.loaded_id(), Some("sir"));
        app.select_model("boids");
        app.report_fault(Fault::refused(STEPPING, "a test fault"));
        assert_eq!(
            app.fault.as_ref().and_then(|shown| shown.model.clone()),
            name(&app, "sir")
        );
        assert_eq!(app.loaded_id(), None, "the faulted model is offloaded");

        app.report_fault(Fault::refused(BUILDING, "a test fault"));
        assert_eq!(
            app.fault.as_ref().and_then(|shown| shown.model.clone()),
            name(&app, "boids"),
            "a build fault is the selected model's"
        );
    }

    #[test]
    fn a_run_with_another_parameter_count_names_its_model() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let declared = app
            .models
            .get("sir")
            .expect("the example models include sir")
            .param_descriptors()
            .len();
        assert_eq!(
            app.open_run(replay_of("sir"), OpenAt::Start),
            Err(format!("This run sets 0 parameters, but model 'sir' has {declared}"))
        );
    }

    #[test]
    fn a_gpu_only_set_without_compute_opens_with_nothing_selected() {
        let mut models = ModelSet::new(henad_core::build_info!());
        for entry in &henad_models::example_models() {
            if entry.metadata().backend == Backend::Gpu {
                models.insert(entry.clone()).expect("example ids are unique");
            }
        }
        assert!(!models.is_empty());
        let Some(app) = AppState::headless(models, false) else {
            return;
        };
        assert_eq!(app.offered_models().count(), 0);
        assert_eq!(app.selected_model, None);
        assert!(app.selected_entry().is_none());
        assert!(app.param_values.is_empty());
        assert!(app.build_setup().is_none(), "Build has nothing to build");
    }

    #[test]
    fn an_opening_of_a_hidden_gpu_model_opens_with_nothing_selected() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let needs_gpu = "Model 'gpu_sir' needs a GPU with compute support, and this device has none";
        let gpu_sir = app
            .models
            .get("gpu_sir")
            .expect("the example models include gpu_sir")
            .clone();

        app.open(AppOpening::Setup {
            setup: gpu_sir.setup(),
            open_at: OpenAt::Start,
        });
        assert_eq!(app.selected_model, None);
        assert!(app.param_values.is_empty());
        assert!(app.sim_thread.is_none());
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.reason.as_str()),
            Some(needs_gpu)
        );
        assert_eq!(app.focus_request, Some(Tab::Model), "the Model panel shows the reason");
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.lead),
            Some("Model not opened")
        );

        let mut replay = replay_of("gpu_sir");
        replay.params = gpu_sir.setup().values().to_vec();
        app.open(AppOpening::Run {
            replay,
            open_at: OpenAt::Tick(5),
        });
        assert_eq!(app.selected_model, None);
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.reason.as_str()),
            Some(needs_gpu)
        );
        assert_eq!(app.run_to_target, None);
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.lead),
            Some("Run not opened")
        );

        app.select_model("sir");
        assert_eq!(app.opening_refusal, None, "a selected model replaces the reason");
    }
}
