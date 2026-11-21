//! State of the app's panels. `DockArea::show_inside` borrows it as its `TabViewer` beside the dock.

use std::sync::Arc;

use egui::TextureHandle;
use henad_compute::cpu::sim_thread::{SimCommand, WakeFn};
use henad_compute::entry::{ModelEntry, ModelLookupError, ModelSet};
use henad_compute::fault::FaultSink;
use henad_compute::simulation::{RunSetup, SetupError};
use henad_compute::snapshot::Snapshot;
use henad_core::action::Schedule;
use henad_core::metadata::Backend;
use henad_core::params::ParamValue;
use henad_core::view::StatsHistory;

use crate::options::Product;
use crate::sim_runner::SimRunner;
use crate::ui::agent_layer::AgentLayer;
use crate::ui::dock::{Layout, Tab};
use crate::ui::edge_layer::EdgeStyle;
use crate::ui::editor::EditorPanel;
use crate::ui::export::Recording;
use crate::ui::export::image::PendingCapture;
#[cfg(test)]
use crate::ui::files::SaveTarget;
use crate::ui::files::{OpenOutcome, SaveOutcome};
use crate::ui::params::{INVALID_SEED, parse_seed};
use crate::ui::results::ResultsPanel;
use crate::ui::sweep::SweepPanel;
use crate::ui::time_axis::RunTime;
use henad_compute::runtime_info::RuntimeInfo;

use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuBatchSettings;

mod build;
mod fault;
mod files;
mod install;
mod open;
mod pool;
mod preview;

#[cfg(test)]
pub use build::STILL_STOPPING;
#[cfg(not(target_arch = "wasm32"))]
pub use build::STOP_WAIT;
pub use build::{BUILDING_MODEL, DID_NOT_STOP};
use build::{PendingBuild, Stopping};
pub use fault::ShownFault;
pub use install::{PendingRebuild, remap_schedule, remap_values, same_code};
pub use open::{OpenAt, OpenedRun, OpeningRefusal};
pub use pool::{ModelPool, UiPool};
pub use preview::PreviewRun;

/// Exponential moving average smoothing factor (0..1, higher = more responsive).
const EMA_ALPHA: f64 = 0.1;

/// Reason shown for the disabled Build button when no model is selected.
pub const NO_SELECTED_MODEL: &str = "Select a model in the Model tab";

/// Default number of snapshots the chart history keeps before it starts dropping the oldest snapshots.
pub const DEFAULT_HISTORY_LEN: usize = 10_000;

/// Default time in milliseconds that a snapshot can spend on a network's layout.
const DEFAULT_LAYOUT_BUDGET_MS: f32 = 4.0;

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
    /// Preview of the Editor's graph model that the live simulation runs, `None` for any other run.
    preview: Option<PreviewRun>,
    /// Id of the model selected when the preview loaded, `None` when no model was selected.
    preview_selection: Option<String>,
    /// Number of builds started that were no preview.
    builds: u64,
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
    /// Run that a modal dialog holds, to go on once the dialog closes, as [`Self::hold_for_modal`] sets it.
    pub held_by_modal: Option<HeldRun>,
    pub run_to_input: u64,
    /// Tab brought to the front once the dock has drawn.
    pub focus_request: Option<Tab>,
    /// Layout the dock shows, copied from the app's workspace before the dock draws. The Sheet pane keys its stored
    /// width on it.
    pub layout: Layout,
    /// Runner of the live simulation. `None` while a build runs, until [`Self::poll_build`] takes the build's runner.
    pub sim_thread: Option<SimRunner>,
    /// Build of the live simulation on its own thread. Always `None` in a browser, where builds run inline.
    building: Option<PendingBuild>,
    /// Whether builds run inline on the calling thread, as they do in a browser. Test builds only.
    #[cfg(test)]
    pub inline_builds: bool,
    /// Saves requested through [`Self::save_as`], each with its target and file name, which a test records instead of
    /// opening a dialog, `None` to open one. [`Self::finish_save`] reports the first. Test builds only.
    #[cfg(test)]
    pub saves_requested: Option<Vec<(SaveTarget, String)>>,
    /// Runs and builds that did not stop in time, kept until their threads end. A build started meanwhile holds them
    /// too, and waits for them.
    stopping: Vec<Arc<Stopping>>,
    /// Pool the app's model code runs in. A fresh pool replaces it once a detached thread has held it past the timeout.
    pools: ModelPool,
    /// Pool the UI thread's parallel work runs in, apart from any model code.
    ui_pool: UiPool,
    /// Generation of the pool the live runner runs in.
    runner_generation: u64,
    /// Sink the live runner, CPU or GPU, stores a panic out of its loop in. A teardown replaces it, so a detached
    /// runner's late panic reaches no later run.
    pub sim_faults: FaultSink,
    pub snapshot: Option<Snapshot>,
    pub sim_running: bool,
    pub grid_texture: Option<TextureHandle>,
    /// Separate from `grid_texture`, so density mode does not overwrite a composite model's field.
    pub density_texture: Option<TextureHandle>,
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
    /// simulation report their device errors into `faults`, and a live runner's panic goes to `sim_faults`. A GPU
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
    pub editor: EditorPanel,
    /// Batch settings of the Pacing tab for a GPU runner.
    pub gpu_batch: GpuBatchSettings,
    /// Whether the window closes, once the unsaved changes dialog has let a close request through.
    #[cfg(not(target_arch = "wasm32"))]
    pub closing: bool,
}

/// Run that a modal dialog holds, as the run was going when the dialog opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldRun {
    /// The run played.
    Playing,
    /// The run ran to this tick.
    RunningTo(u64),
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
            preview: None,
            preview_selection: None,
            builds: 0,
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
            held_by_modal: None,
            run_to_input: 0,
            focus_request: None,
            layout: Layout::default(),
            sim_thread: None,
            building: None,
            #[cfg(test)]
            inline_builds: false,
            #[cfg(test)]
            saves_requested: Some(Vec::new()),
            stopping: Vec::new(),
            pools,
            ui_pool: UiPool::default(),
            runner_generation: 0,
            sim_faults: FaultSink::new(),
            snapshot: None,
            sim_running: false,
            grid_texture: None,
            density_texture: None,
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
            editor: EditorPanel::default(),
            gpu_batch: GpuBatchSettings::default(),
            #[cfg(not(target_arch = "wasm32"))]
            closing: false,
        }
    }

    /// Tears down the live simulation and builds the selected model from the fields the next build reads.
    ///
    /// Does nothing when [`Self::build_setup`] rejects the fields, the device cannot hold the selection at them, or the
    /// Seed field holds text that is no seed. Build is disabled then, with the reason [`Self::build_refusal`] returns.
    pub fn reset_simulation(&mut self) {
        let setup = match self.build_setup() {
            Some(Ok(setup)) => Some(setup),
            Some(Err(error)) => {
                log::warn!("Build refused: {}", setup_message(&error));
                return;
            }
            None => None,
        };
        // The engine asserts the same check, and in a browser its panic ends the page.
        if let Some(message) = self.selection_too_large() {
            log::warn!("Build refused: {message}");
            return;
        }
        // The seed the last valid text set is not the one the Seed field shows.
        if parse_seed(&self.seed_text).is_err() {
            log::warn!("Build refused: {INVALID_SEED}");
            return;
        }
        self.load(setup);
    }

    /// Tears down the live simulation and builds `setup`. The setup's entry then becomes the loaded entry, and `None`
    /// tears down alone.
    ///
    /// The loaded fields are the setup's from the start of the build, and commands sent while it runs wait for its
    /// runner. A build that fails goes to the fault modal, or to the Editor for a fault of the code the Editor
    /// applied, and leaves no model loaded.
    fn load(&mut self, setup: Option<RunSetup>) {
        self.load_setup(setup, None);
    }

    /// Tears down the live simulation and builds `setup`, as the preview `preview` when set.
    fn load_setup(&mut self, setup: Option<RunSetup>, preview: Option<PreviewRun>) {
        self.settle_opened_run(setup.as_ref());
        self.clear_run();
        self.preview = preview;
        self.ticks_per_snapshot = 1;
        self.export_status = None;

        let Some(setup) = setup else {
            return;
        };

        self.stats_history = Some(StatsHistory::new(
            setup.entry().stat_descriptors().to_vec(),
            self.history_capacity,
        ));
        self.sim_running = false;
        if preview.is_none() {
            self.builds += 1;
        }
        // The fields of the next build are the setup's only for the selected model. A rebuild of a model loaded
        // while another one is selected leaves them alone, and so does a preview.
        if preview.is_none() && self.selected_model.as_deref() == Some(setup.entry().id()) {
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

    /// Steps the loaded model uncapped to `tick` and pauses there.
    ///
    /// A tick behind the current one rebuilds first, as Build does: the preview of the Editor's sheet replays its start
    /// while [`Self::build_replays_preview`] holds, and otherwise the selected model is built. Note that the selected
    /// model is the loaded one only while [`Self::selection_is_loaded`] holds. A rebuild that Build refuses, as for a
    /// Seed field that holds no seed, leaves the run where it is.
    pub fn run_to(&mut self, tick: u64) {
        self.held_by_modal = None;
        if tick < self.current_tick() {
            crate::ui::playback::press_build(self);
            // A build clears the snapshot. Otherwise the runner, still past the tick, would publish again at once, and
            // the next snapshot would run to the tick again.
            if tick < self.current_tick() {
                return;
            }
        }
        if !self.has_simulation() {
            return;
        }
        self.send_command(SimCommand::RunTo(tick));
        self.sim_running = false;
        self.run_to_target = Some(tick);
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

    /// Plays the live simulation, and ends a run to a tick.
    pub fn play(&mut self) {
        self.held_by_modal = None;
        if !self.has_simulation() {
            return;
        }
        self.send_command(SimCommand::Play);
        self.sim_running = true;
        self.run_to_target = None;
    }

    /// Steps the live simulation once, and ends a run to a tick.
    pub fn step_once(&mut self) {
        self.held_by_modal = None;
        if !self.has_simulation() {
            return;
        }
        self.send_command(SimCommand::StepOnce);
        self.run_to_target = None;
    }

    /// Holds the live run while a modal dialog is open, and lets it go on once the dialog closes. `modal_open` is
    /// whether a modal dialog showed in the last frame.
    ///
    /// A run that plays, or runs to a tick, pauses as a dialog opens, and plays or runs to its tick again once no
    /// dialog is open. A press of Play, Step, Pause or Run, a build and a teardown in the meantime end the hold, and
    /// the run then stays as they leave it. Frames keep coming while a run is held.
    pub fn hold_for_modal(&mut self, modal_open: bool) {
        if modal_open {
            if self.held_by_modal.is_none() && self.has_simulation() {
                let held = match self.run_to_target {
                    Some(target) => Some(HeldRun::RunningTo(target)),
                    None if self.sim_running => Some(HeldRun::Playing),
                    None => None,
                };
                if let Some(held) = held {
                    self.pause_simulation();
                    self.held_by_modal = Some(held);
                }
            }
            // The run goes on in the frame after the dialog closes. A key that closes it asks for no later frame.
            if self.held_by_modal.is_some() {
                self.egui_ctx.request_repaint();
            }
            return;
        }
        match self.held_by_modal.take() {
            Some(HeldRun::Playing) => self.play(),
            // A run that reached its tick before the pause has nothing left to run.
            Some(HeldRun::RunningTo(target)) if target > self.current_tick() => self.run_to(target),
            Some(HeldRun::RunningTo(_)) | None => {}
        }
    }

    /// Returns whether a close request of the window waits. It waits while a model the Editor holds has unsaved edits,
    /// and the unsaved changes dialog asks about them, as before a replacement of the models.
    ///
    /// Once the dialog closes the window, [`Self::closing`] holds and no close request waits.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn hold_close(&mut self) -> bool {
        if self.closing || self.editor.unsaved_names().is_empty() {
            return false;
        }
        crate::ui::editor::replace_model(self, crate::ui::editor::Replacement::CloseWindow);
        true
    }

    /// Closes the window, which no close request then holds.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn close_window(&mut self) {
        self.closing = true;
        self.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Pauses the live simulation, a run to a tick included. A run that a modal dialog holds stays paused once the
    /// dialog closes.
    pub fn pause_simulation(&mut self) {
        self.held_by_modal = None;
        if !self.has_simulation() {
            return;
        }
        if self.sim_running || self.run_to_target.is_some() {
            self.send_command(SimCommand::Pause);
        }
        self.sim_running = false;
        self.run_to_target = None;
    }

    /// Tears down the live simulation, or abandons the build that runs, and leaves no model loaded.
    ///
    /// Neither waits past [`STOP_WAIT`]. A runner or a build still running then is detached.
    pub fn offload_simulation(&mut self) {
        self.clear_run();
        self.sim_running = false;
        self.stats_history = None;
        self.opened_run = None;
    }

    /// Stops the recording, tears down the live simulation, and clears the run's snapshot, textures, loaded entry,
    /// preview, pending rebuild, run to a tick and agent population.
    ///
    /// The recording stops first. It ends at the snapshot's tick.
    fn clear_run(&mut self) {
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
        self.loaded = None;
        self.preview = None;
        self.pending_rebuild = None;
        self.run_to_target = None;
        self.held_by_modal = None;
        // The next model may have no agents at all.
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
    /// same code, as [`same_code`] compares it.
    ///
    /// Note that a model whose code the set has replaced since its build is not the selection, and live edits and
    /// actions do not reach it. Neither is the Editor's preview.
    pub fn selection_is_loaded(&self) -> bool {
        self.preview.is_none()
            && self.loaded.as_ref().is_some_and(|loaded| {
                self.selected_entry()
                    .is_some_and(|selected| selected.id() == loaded.id() && same_code(loaded, selected))
            })
    }

    /// Returns whether the live simulation runs the selected model under code that the set has replaced since its
    /// build. Always false for the Editor's preview.
    pub fn code_pending(&self) -> bool {
        self.preview.is_none()
            && self.loaded.as_ref().is_some_and(|loaded| {
                self.selected_entry()
                    .is_some_and(|selected| selected.id() == loaded.id() && !same_code(loaded, selected))
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

    /// Returns the unit and tick length of the loaded run, `None` with no model loaded or for a model with no time
    /// scale.
    pub fn loaded_time(&self) -> Option<RunTime> {
        let entry = self.loaded.as_ref()?;
        let time = entry.metadata().time.as_ref()?;
        let tick_length = time.tick_length_in(entry.param_descriptors(), &self.loaded_values)?;
        Some(RunTime {
            unit: time.unit,
            tick_length,
        })
    }

    /// Tick of the latest snapshot, 0 before the first.
    pub fn current_tick(&self) -> u64 {
        self.snapshot.as_ref().map_or(0, |snapshot| snapshot.tick)
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
    /// loaded model runs with when `id` is the loaded model and the run is no preview.
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
        let previewing = self.preview.is_some();
        if let Some(loaded) = self.loaded.as_ref().filter(|loaded| loaded.id() == id && !previewing) {
            let current = self.models.get(id).unwrap_or(loaded);
            let values = remap_values(
                loaded.param_descriptors(),
                &self.loaded_values,
                current.param_descriptors(),
            )
            .values;
            let schedule = remap_schedule(&self.loaded_schedule, current.action_descriptors()).schedule;
            self.set_param_values(values);
            self.set_seed(self.loaded_seed);
            self.schedule = schedule;
            return;
        }
        self.set_param_values(self.models.get(id).map(default_values).unwrap_or_default());
        // Entries index the previous model's actions.
        self.schedule = Schedule::default();
    }

    /// Sets the values of the next build to `values`, none of them marked as awaiting a build.
    pub fn set_param_values(&mut self, values: Vec<ParamValue>) {
        self.pending_reload = vec![false; values.len()];
        self.param_values = values;
    }

    /// Sets the seed of the next build to `seed`, and the Seed field to its digits, empty for the default seed.
    pub fn set_seed(&mut self, seed: Option<u64>) {
        self.seed = seed;
        self.seed_text = seed.map(|seed| seed.to_string()).unwrap_or_default();
    }

    /// Sends a live edit of parameter `index` to the loaded model, and records `value` as a value that the model
    /// runs with.
    ///
    /// Returns whether the edit was sent. It is sent only while the selection is the loaded model.
    pub fn send_live_param(&mut self, index: usize, value: ParamValue) -> bool {
        if !self.selection_is_loaded() || !self.has_simulation() {
            return false;
        }
        self.send_param(index, value);
        true
    }

    /// Sends a live edit of parameter `index` to the live simulation, and records `value` among the values it runs
    /// with.
    fn send_param(&mut self, index: usize, value: ParamValue) {
        self.send_command(SimCommand::SetParam {
            index,
            value: value.clone(),
        });
        if let Some(slot) = self.loaded_values.get_mut(index) {
            *slot = value;
        }
    }

    /// Reasons this machine cannot build the selection. Always empty for a CPU model.
    pub fn selection_shortfalls(&self) -> Vec<String> {
        self.selected_entry()
            .map_or_else(Vec::new, |entry| self.entry_shortfalls(entry, &self.param_values))
    }

    /// Reasons this machine cannot build `entry` at `values`.
    ///
    /// When the Editor's sheet builds `entry`, work past a budget is named by the sheet's lines, as
    /// [`henad_graph::sheet::SheetFile::shortfalls`] words it, and no lowered pass is named.
    pub fn entry_shortfalls(&self, entry: &ModelEntry, values: &[ParamValue]) -> Vec<String> {
        let limits = &self.runtime.granted;
        match self.editor_sheet_of(entry) {
            Some(file) => file.shortfalls(entry, values, limits),
            None => entry.shortfalls(values, limits),
        }
    }

    /// Returns Build's disabled reason when this machine cannot build `entry` at `values`, `None` when it can.
    pub fn too_large(&self, entry: &ModelEntry, values: &[ParamValue]) -> Option<String> {
        let shortfalls = self.entry_shortfalls(entry, values);
        (!shortfalls.is_empty()).then(|| too_large_message(&shortfalls))
    }

    /// Returns Build's disabled reason when this machine cannot build the selection, `None` when it can or nothing
    /// is selected.
    pub fn selection_too_large(&self) -> Option<String> {
        self.too_large(self.selected_entry()?, &self.param_values)
    }

    /// Sheet of the Editor when it builds `entry`, `None` for an entry of other code.
    pub fn editor_sheet_of(&self, entry: &ModelEntry) -> Option<&henad_graph::sheet::SheetFile> {
        self.editor
            .model
            .sheet()
            .and_then(|sheet| sheet.file())
            .filter(|file| entry.source().code_hash() == Some(file.code().hash()))
    }

    /// Returns the reason Build cannot build the selection, or `None` when it can.
    ///
    /// Build is disabled with the reason as its tooltip. The reasons are no selection, a size the device cannot
    /// hold, a Seed field that does not parse and values that [`Self::build_setup`] rejects, in that order.
    pub fn build_refusal(&self) -> Option<String> {
        if self.selected_entry().is_none() {
            return Some(NO_SELECTED_MODEL.to_owned());
        }
        if let Some(message) = self.selection_too_large() {
            return Some(message);
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

    /// Takes the newest snapshot of the live simulation, if one has arrived, and receives it through
    /// [`Self::receive_snapshot`].
    pub fn poll_snapshot(&mut self) {
        if let Some(snapshot) = self.sim_thread.as_mut().and_then(SimRunner::take_snapshot) {
            self.receive_snapshot(snapshot);
        }
    }

    /// Makes `snapshot` the snapshot the panels show, after adding its stats to the history and the recording.
    ///
    /// A snapshot at or past the run to a tick ends it, and one past it runs to the tick again, which rebuilds. The
    /// previous snapshot goes back to the runner, to be refilled.
    pub fn receive_snapshot(&mut self, snapshot: Snapshot) {
        // The loop can be past the target by the time it handles `RunTo`, and then pauses where it is. A second
        // run to the target sees this snapshot's tick, rebuilds, and stops there.
        let passed_target = self
            .run_to_target
            .filter(|&target| snapshot.tick > target && (self.selection_is_loaded() || self.build_replays_preview()));
        if self.run_to_target.is_some_and(|target| snapshot.tick >= target) {
            self.run_to_target = None;
        }
        // A publish at the newest row's tick replaces that row. After an action it carries the
        // action's effect, and after a paused layout step the same stats again.
        if let Some(history) = &mut self.stats_history {
            for sample in &snapshot.run_to_samples {
                history.push_entries(sample.entries(), sample.tick());
            }
            history.push_entries(&snapshot.stats, snapshot.tick);
        }
        self.record(&snapshot);
        // Passing the outgoing snapshot back lets the sim thread refill it instead of allocating.
        if let Some(previous) = self.snapshot.replace(snapshot)
            && let Some(thread) = &mut self.sim_thread
        {
            thread.recycle(previous);
        }
        if let Some(target) = passed_target {
            self.run_to(target);
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
    crate::ui::text::capitalize(&error.to_string())
}

/// Returns `shortfalls`, the reasons the device cannot build a model, as Build's disabled reason.
fn too_large_message(shortfalls: &[String]) -> String {
    format!("Too large for this device: {}", shortfalls.join("; "))
}

/// Returns `error` as Build's disabled reason, as in `Parameter 'infection_rate': 2 is outside the range 0 to 1`.
pub fn setup_message(error: &SetupError) -> String {
    use crate::ui::text::describe_error;
    match error {
        SetupError::Param(reason) => describe_error(reason),
        other => describe_error(other),
    }
}

#[cfg(test)]
mod tests {
    use henad_core::params::ParamValue;
    use henad_explore::output::details::{ChoiceForm, params_by_id_json};

    use super::{AppState, HeldRun, OpenAt};
    use crate::tests::support::{gpu_only_app, set_grid_side};
    use crate::ui::export::metadata::run_details;
    use crate::ui::modal_shown;

    /// Picking the loaded model again restores its values. Otherwise the run details would record values the model
    /// was not built with.
    #[test]
    fn picking_the_loaded_model_again_restores_what_it_runs_with() {
        let Some(mut app) = AppState::headless(henad_models::example_models(), false) else {
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

    /// A modal dialog holds a playing run or a run to a tick, and the run goes on once the dialog closes. A pause or a
    /// build while the dialog is open ends the hold.
    #[test]
    fn a_modal_dialog_holds_the_run_until_it_closes() {
        let Some(mut app) = AppState::headless(henad_models::example_models(), false) else {
            return;
        };
        app.select_model("sir");
        set_grid_side(&mut app, 64);
        app.reset_simulation();
        app.wait_for_build();

        app.play();
        app.hold_for_modal(true);
        assert!(!app.sim_running, "the dialog holds the run");
        assert_eq!(app.held_by_modal, Some(HeldRun::Playing));
        app.hold_for_modal(true);
        assert_eq!(
            app.held_by_modal,
            Some(HeldRun::Playing),
            "a second frame changes nothing"
        );
        app.hold_for_modal(false);
        assert!(app.sim_running, "the run plays on");
        assert_eq!(app.held_by_modal, None);

        app.pause_simulation();
        app.run_to(1_000_000);
        app.hold_for_modal(true);
        assert_eq!(app.run_to_target, None);
        app.hold_for_modal(false);
        assert_eq!(app.run_to_target, Some(1_000_000), "the run to the tick goes on");

        app.hold_for_modal(true);
        app.pause_simulation();
        app.hold_for_modal(false);
        assert!(!app.sim_running && app.run_to_target.is_none(), "Pause ends the hold");

        app.play();
        app.hold_for_modal(true);
        app.reset_simulation();
        app.hold_for_modal(false);
        assert!(!app.sim_running, "a build ends the hold");

        app.hold_for_modal(true);
        app.hold_for_modal(false);
        assert!(!app.sim_running, "a paused run stays paused");
    }

    /// A modal dialog drawn in one frame holds the run from the next frame, as `HenadApp::logic` reads it, and the run
    /// goes on in the frame after the first frame that draws no dialog.
    #[test]
    fn a_dialog_drawn_in_a_frame_holds_the_run_from_the_next() {
        let Some(mut app) = AppState::headless(henad_models::example_models(), false) else {
            return;
        };
        app.select_model("sir");
        set_grid_side(&mut app, 64);
        app.reset_simulation();
        app.wait_for_build();
        app.play();
        let ctx = egui::Context::default();
        let frame = |app: &mut AppState, dialog: bool| {
            app.hold_for_modal(modal_shown(&ctx));
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                if dialog {
                    egui::Modal::new(egui::Id::new("henad_test_modal")).show(ui.ctx(), |ui| ui.label("A dialog"));
                }
            });
            output.textures_delta.clear();
        };

        frame(&mut app, true);
        assert!(app.sim_running, "the frame that draws the dialog");
        frame(&mut app, false);
        assert!(!app.sim_running, "the next frame holds the run");
        frame(&mut app, false);
        assert!(app.sim_running, "the run goes on once a frame drew no dialog");
    }

    #[test]
    fn a_gpu_only_set_without_compute_opens_with_nothing_selected() {
        let Some(app) = gpu_only_app() else {
            return;
        };
        assert_eq!(app.offered_models().count(), 0);
        assert_eq!(app.selected_model, None);
        assert!(app.selected_entry().is_none());
        assert!(app.param_values.is_empty());
        assert!(app.build_setup().is_none(), "Build has nothing to build");
    }
}
