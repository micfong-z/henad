//! Checked setups for one build of a model, and the simulations they build.
//!
//! A [`RunSetup`] holds a model's parameter values by id, a seed and scheduled actions, each checked against the
//! model's descriptors when set. [`RunSetup::build`] returns a [`Simulation`], one built model that its caller steps,
//! and a [`StatSample`] is one sample of the model's stats, read by label.
//!
//! Actions fire under [`Fire::AfterStep`]. Tick 0's fire inside the build, and each later tick's after the step that
//! reaches it. Every call returns a model's panic or a device error as a [`Fault`].
//!
//! On native targets a CPU model's [`RunSetup::build`], [`Simulation::step`], [`Simulation::run_for`],
//! [`Simulation::run_to`], [`Simulation::run_sampled`], [`Simulation::stats`], [`Simulation::act`],
//! [`Simulation::views`] and [`Simulation::relax_layout`] enter the current rayon pool once each, and
//! [`Simulation::write_state`] enters it for its view preparation alone. [`Simulation::set_param`] runs no parallel
//! pass and enters no pool.

use std::fmt;
use std::io::{self, Write};
use std::ops::ControlFlow;

#[cfg(not(target_arch = "wasm32"))]
use henad_core::action::Fire;
use henad_core::action::{RefusedActions, Schedule, Scheduled};
use henad_core::explore::replay::Replay;
use henad_core::explore::value::{ValueError, check_value, parse_value};
use henad_core::export::state as state_export;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::send_sync::WasmNotSend;
use henad_core::view::{EdgeView, GridView, PointView, StatEntry, StatValue};

use crate::entry::{ModelEntry, ModelState};
use crate::fault::{BUILDING, Fault, STEPPING, catching};
use crate::gpu::GpuContext;
#[cfg(not(target_arch = "wasm32"))]
use crate::gpu::{GpuSimState, fault::catching_on, stepping};

/// Values, seed and schedule for one build of an entry, each checked against its descriptors when set.
///
/// [`ModelEntry::setup`] returns one at the declared defaults, the default seed and no actions.
#[derive(Debug, Clone)]
pub struct RunSetup {
    entry: ModelEntry,
    /// One value per parameter in descriptor order.
    values: Vec<ParamValue>,
    seed: Option<u64>,
    schedule: Schedule,
}

impl RunSetup {
    /// Returns a setup of `entry` at its declared defaults, the default seed and no actions.
    pub(crate) fn new(entry: ModelEntry) -> Self {
        let values = entry
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        Self {
            entry,
            values,
            seed: None,
            schedule: Schedule::default(),
        }
    }

    /// Returns the setup with parameter `id` set to `value`.
    ///
    /// `value` is an `f32`, a `u32`, a `bool`, or a [`ParamValue`] itself, as `ParamValue::Choice(index)` is for a
    /// choice parameter. A `u32` never stands in for a choice index.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::Param`] for an unknown id, a value of another kind, or a value out of bounds.
    pub fn set(mut self, id: &str, value: impl Into<ParamValue>) -> Result<Self, SetupError> {
        let value = value.into();
        let index = param_index(&self.entry, id)?;
        check_value(&self.entry.param_descriptors()[index].kind, &value).map_err(|error| param_error(id, error))?;
        self.values[index] = value;
        Ok(self)
    }

    /// Returns the setup with parameter `id` read from `text`, as `--set` reads it. A choice reads an option's name.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::Param`] for an unknown id, text that does not read as the parameter's kind, or a value
    /// out of bounds.
    pub fn set_text(mut self, id: &str, text: &str) -> Result<Self, SetupError> {
        let index = param_index(&self.entry, id)?;
        let value =
            parse_value(&self.entry.param_descriptors()[index].kind, text).map_err(|error| param_error(id, error))?;
        self.values[index] = value;
        Ok(self)
    }

    /// Returns the setup with `seed` in place of the model's default seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Returns the setup with action `action_id` scheduled at `tick`. Two actions due at one tick fire in the order
    /// they were scheduled.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::UnknownAction`] for an id the model does not declare.
    pub fn act_at(mut self, action_id: &str, tick: u64) -> Result<Self, SetupError> {
        let index = action_index(&self.entry, action_id)?;
        let mut entries = self.schedule.entries().to_vec();
        entries.push(Scheduled {
            index,
            id: action_id.to_owned(),
            tick,
        });
        self.schedule = Schedule::from_entries(entries);
        Ok(self)
    }

    /// Returns the setup a recorded run was built from.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::WrongModel`] for a run of another model, and the errors of [`Self::from_parts`] for
    /// values or actions the entry refuses.
    pub fn from_replay(entry: &ModelEntry, replay: &Replay) -> Result<Self, SetupError> {
        if replay.model != entry.id() {
            return Err(SetupError::WrongModel {
                expected: entry.id().to_owned(),
                found: replay.model.clone(),
            });
        }
        Self::from_parts(entry, &replay.params, Some(replay.seed), replay.schedule.clone())
    }

    /// Returns a setup holding `values`, `seed` and `schedule`, each checked as [`Self::set`] and [`Self::act_at`]
    /// check them.
    ///
    /// `values` holds one value per parameter in descriptor order, the engine-prepended ones first, as
    /// [`Self::values`] returns them.
    ///
    /// # Errors
    ///
    /// Returns a [`SetupError`] for a value count other than the entry's parameter count, a value of another kind or
    /// out of bounds, or a schedule entry naming no action of the entry.
    pub fn from_parts(
        entry: &ModelEntry,
        values: &[ParamValue],
        seed: Option<u64>,
        schedule: Schedule,
    ) -> Result<Self, SetupError> {
        let descriptors = entry.param_descriptors();
        if values.len() != descriptors.len() {
            return Err(SetupError::ParamCount {
                expected: descriptors.len(),
                found: values.len(),
            });
        }
        for (descriptor, value) in descriptors.iter().zip(values) {
            check_value(&descriptor.kind, value).map_err(|error| param_error(descriptor.id, error))?;
        }
        let actions = entry.action_descriptors();
        for scheduled in schedule.entries() {
            if actions
                .get(scheduled.index)
                .is_none_or(|action| action.id != scheduled.id)
            {
                return Err(SetupError::UnknownAction {
                    id: scheduled.id.clone(),
                });
            }
        }
        Ok(Self {
            entry: entry.clone(),
            values: values.to_vec(),
            seed,
            schedule,
        })
    }

    pub fn entry(&self) -> &ModelEntry {
        &self.entry
    }

    /// One value per parameter in descriptor order, the engine-prepended ones first.
    pub fn values(&self) -> &[ParamValue] {
        &self.values
    }

    /// Seed of the build. `None` is the model's default seed, and no `Some` value reproduces it.
    pub fn seed(&self) -> Option<u64> {
        self.seed
    }

    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// Builds the model, on `gpu` for a GPU entry, then fires the schedule's tick-0 entries.
    ///
    /// The returned simulation already holds tick 0's actions, and records tick 0 as fired.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the build or a tick-0 action panics, the device refuses either, a GPU entry has no
    /// device, or a GPU entry is built on wasm32.
    pub fn build(&self, gpu: Option<&GpuContext>) -> Result<Simulation, Fault> {
        #[cfg(target_arch = "wasm32")]
        if self.entry.gpu_needs().is_some() {
            return Err(gpu_in_browser(self.entry.id()));
        }
        let schedule = &self.schedule;
        let engine = match self.entry.build(&self.values, self.seed, gpu)? {
            ModelState::Cpu(mut state) => {
                if !schedule.is_empty() {
                    cpu_call(BUILDING, || refusal(&schedule.run_due(&mut *state), BUILDING))?;
                }
                Engine::Cpu(state)
            }
            #[cfg(not(target_arch = "wasm32"))]
            ModelState::Gpu(mut state) => {
                let ctx = gpu
                    .ok_or_else(|| Fault::refused(BUILDING, "a GPU model was built with no device"))?
                    .clone();
                if !schedule.is_empty() {
                    catching_on(&ctx, BUILDING, || {
                        refusal(&stepping::run_due(&mut *state, &ctx, schedule), BUILDING)?;
                        stepping::wait(&ctx)
                    })??;
                }
                Engine::Gpu { state, ctx }
            }
            #[cfg(target_arch = "wasm32")]
            ModelState::Gpu(_) => return Err(gpu_in_browser(self.entry.id())),
        };
        Ok(Simulation {
            setup: self.clone(),
            engine,
        })
    }
}

/// Returns the position of parameter `id` in the descriptors of `entry`.
fn param_index(entry: &ModelEntry, id: &str) -> Result<usize, SetupError> {
    entry.param_index(id).ok_or_else(|| {
        SetupError::Param(ValueError::UnknownParam {
            id: id.to_owned(),
            known: entry
                .param_descriptors()
                .iter()
                .map(|descriptor| descriptor.id)
                .collect(),
        })
    })
}

/// Returns the position of action `id` in the descriptors of `entry`.
fn action_index(entry: &ModelEntry, id: &str) -> Result<usize, SetupError> {
    entry
        .action_index(id)
        .ok_or_else(|| SetupError::UnknownAction { id: id.to_owned() })
}

/// Returns the error for a value the descriptor of parameter `id` refuses.
fn param_error(id: &str, error: ValueError) -> SetupError {
    SetupError::Param(ValueError::Param {
        id: id.to_owned(),
        source: Box::new(error),
    })
}

/// Returns the fault for a scheduled action the state refused.
///
/// Every engine refuses only an index past its model's actions, and a setup checks each id before it stores an entry.
/// A refusal is therefore an engine contract violation.
fn refusal(refused: &RefusedActions<'_>, during: &'static str) -> Result<(), Fault> {
    match refused.first() {
        None => Ok(()),
        Some(action) => Err(Fault::refused(
            during,
            format!(
                "the model refused its own action '{}' at tick {}",
                action.id, action.tick
            ),
        )),
    }
}

/// Returns the fault for a GPU entry built in a browser.
#[cfg(target_arch = "wasm32")]
fn gpu_in_browser(id: &str) -> Fault {
    Fault::refused(
        BUILDING,
        format!("model '{id}' runs on the GPU, and a browser steps a GPU model through GpuSimThread alone"),
    )
}

/// Runs `task` inside the current rayon pool and catches a panic out of it.
fn cpu_call<T: WasmNotSend>(
    during: &'static str,
    task: impl FnOnce() -> Result<T, Fault> + WasmNotSend,
) -> Result<T, Fault> {
    in_pool(|| catching(during, task)?)
}

/// Runs `task` inside the current rayon pool.
///
/// Called from outside the pool, the body runs on a worker. Each parallel pass then starts from a worker instead of
/// being injected from outside and parking the caller.
#[cfg(not(target_arch = "wasm32"))]
fn in_pool<T: Send>(task: impl FnOnce() -> T + Send) -> T {
    rayon::scope(|_| task())
}

/// Runs `task` on the calling thread. The frame driver pumps from outside the pool too.
#[cfg(target_arch = "wasm32")]
fn in_pool<T>(task: impl FnOnce() -> T) -> T {
    task()
}

/// Reason a setup, a live edit or a live action was refused.
#[derive(Debug)]
#[non_exhaustive]
pub enum SetupError {
    /// An unknown parameter id, or a value its descriptor refuses.
    Param(ValueError),
    /// [`RunSetup::from_parts`] was handed a value count other than the entry's parameter count.
    ParamCount { expected: usize, found: usize },
    /// An action id the model does not declare.
    UnknownAction { id: String },
    /// A live edit of a parameter that applies only on a rebuild.
    ReloadOnly { id: String },
    /// A recorded run of model `found`, handed to an entry of model `expected`.
    WrongModel { expected: String, found: String },
    /// Model code panicked, or the device refused an action pass, during a live edit or action.
    Fault(Fault),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Param(_) => f.write_str("cannot set the parameter"),
            Self::ParamCount { expected, found } => {
                write!(f, "expected {expected} parameter values, found {found}")
            }
            Self::UnknownAction { id } => write!(f, "model has no action '{id}'"),
            Self::ReloadOnly { id } => write!(f, "parameter '{id}' applies only when the model is built"),
            Self::WrongModel { expected, found } => {
                write!(f, "the run is of model '{found}', not '{expected}'")
            }
            Self::Fault(_) => f.write_str("the model faulted"),
        }
    }
}

impl std::error::Error for SetupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Param(error) => Some(error),
            Self::Fault(fault) => Some(fault),
            Self::ParamCount { .. }
            | Self::UnknownAction { .. }
            | Self::ReloadOnly { .. }
            | Self::WrongModel { .. } => None,
        }
    }
}

impl From<Fault> for SetupError {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault)
    }
}

/// State a [`Simulation`] steps, with the device a GPU state lives on.
enum Engine {
    Cpu(Box<dyn SimState>),
    #[cfg(not(target_arch = "wasm32"))]
    Gpu {
        state: Box<dyn GpuSimState>,
        ctx: GpuContext,
    },
}

impl Engine {
    fn state(&self) -> &dyn SimState {
        match self {
            Self::Cpu(state) => &**state,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Gpu { state, .. } => &**state,
        }
    }
}

/// One built model and its schedule, stepped by its caller.
///
/// On native targets a CPU simulation runs the model code of each stepping, sampling, view, layout and action call
/// inside one `rayon::scope` on the current pool, and `pool.install(|| simulation.run_for(n))` picks the pool.
/// [`Self::set_param`] stays on the calling thread. A GPU simulation runs each call on the calling thread, inside wgpu
/// error scopes that keep a device error on the call that raised it.
///
/// Note that an error no scope catches, such as a lost device, lands in the context's
/// [`FaultSink`](crate::fault::FaultSink), and whichever holder of the context waits next reports it. Give each thread
/// its own [`GpuContext`] when several simulations step at once.
pub struct Simulation {
    setup: RunSetup,
    engine: Engine,
}

// A host steps a simulation on a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
const _: fn() = || {
    fn send<T: Send>() {}
    send::<Simulation>();
};

/// Prints the setup and the tick, and leaves out the state.
impl fmt::Debug for Simulation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Simulation")
            .field("setup", &self.setup)
            .field("tick", &self.tick())
            .finish_non_exhaustive()
    }
}

impl Simulation {
    /// Setup the simulation was built from. Live [`Self::set_param`] edits and immediate [`Self::act`] calls leave it
    /// unchanged.
    pub fn setup(&self) -> &RunSetup {
        &self.setup
    }

    pub fn tick(&self) -> u64 {
        self.engine.state().tick()
    }

    pub fn population(&self) -> u64 {
        self.engine.state().population()
    }

    /// Approximate heap use of the state, and only what it owns. A GPU state's buffers live on the device.
    pub fn heap_bytes(&self) -> usize {
        self.engine.state().heap_bytes()
    }

    /// Number of jobs one step splits into. `None` for a backend with no such split.
    pub fn parallel_jobs(&self) -> Option<usize> {
        self.engine.state().parallel_jobs()
    }

    /// Steps one tick. Same as `run_for(1)`.
    ///
    /// Note that each call enters the rayon pool once on the CPU and waits for the device once on the GPU. A loop of
    /// `step()` pays that per tick, where [`Self::run_for`] pays it once. A loop of `step()` from outside the pool
    /// belongs inside `rayon::scope` or `pool.install`.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the model panics or the device reports an error.
    pub fn step(&mut self) -> Result<(), Fault> {
        self.run_for(1)
    }

    /// Steps `ticks` ticks.
    ///
    /// Note that without the template's profile block, a debug build runs the kernels at opt-level 0 in the crate that
    /// registers the model, or in henad-models for an example entry, and its timings mean little. `--release` is the
    /// measured configuration.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the model panics or the device reports an error.
    pub fn run_for(&mut self, ticks: u64) -> Result<(), Fault> {
        let end = self.tick().saturating_add(ticks);
        self.advance(end)
    }

    /// Steps up to `tick` and never past it. A tick at or behind the current one steps nothing.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the model panics or the device reports an error.
    pub fn run_to(&mut self, tick: u64) -> Result<(), Fault> {
        self.advance(tick)
    }

    /// Runs to `end_tick`, calling `on_sample` at the current tick, at each later multiple of `interval`, and at
    /// `end_tick`.
    ///
    /// Returns the `Break` of the first sample that asks to stop, or `Continue` once `end_tick` is reached. Each
    /// sample is taken as [`Self::stats`] takes it.
    ///
    /// On native targets a CPU model enters the pool once for the whole call, and `on_sample` runs inside it on a
    /// pool worker. It needs `Send`, and it holds that worker while it runs. A GPU model calls it on the calling
    /// thread. Either way `on_sample` runs outside the fault scopes, and a panic in it unwinds as the caller's own.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the model panics or the device reports an error.
    ///
    /// # Panics
    ///
    /// Panics when `interval` is 0.
    pub fn run_sampled<B: WasmNotSend>(
        &mut self,
        end_tick: u64,
        interval: u64,
        mut on_sample: impl FnMut(&StatSample) -> ControlFlow<B> + WasmNotSend,
    ) -> Result<ControlFlow<B>, Fault> {
        assert!(interval > 0, "a sampling interval is at least one tick");
        let Self { setup, engine } = self;
        let schedule = &setup.schedule;
        match engine {
            // One pool entry for the whole call. A pool entry per sample costs more than a small model's tick.
            Engine::Cpu(state) => in_pool(|| {
                let first = catching(STEPPING, || sample_cpu(&mut **state))?;
                if let ControlFlow::Break(stop) = on_sample(&first) {
                    return Ok(ControlFlow::Break(stop));
                }
                while state.tick() < end_tick {
                    let next = next_sample_tick(state.tick(), interval, end_tick);
                    let sample = catching(STEPPING, || {
                        step_cpu(&mut **state, schedule, next)?;
                        Ok(sample_cpu(&mut **state))
                    })??;
                    if let ControlFlow::Break(stop) = on_sample(&sample) {
                        return Ok(ControlFlow::Break(stop));
                    }
                }
                Ok(ControlFlow::Continue(()))
            }),
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { state, ctx } => {
                let first = sample_gpu(&mut **state, ctx)?;
                if let ControlFlow::Break(stop) = on_sample(&first) {
                    return Ok(ControlFlow::Break(stop));
                }
                while state.tick() < end_tick {
                    let next = next_sample_tick(state.tick(), interval, end_tick);
                    advance_gpu(&mut **state, ctx, schedule, next)?;
                    let sample = sample_gpu(&mut **state, ctx)?;
                    if let ControlFlow::Break(stop) = on_sample(&sample) {
                        return Ok(ControlFlow::Break(stop));
                    }
                }
                Ok(ControlFlow::Continue(()))
            }
        }
    }

    /// Samples as a sweep track does: `prepare_view` first on the CPU, a blocking stats-only readback on the GPU.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the model panics or the device reports an error.
    pub fn stats(&mut self) -> Result<StatSample, Fault> {
        match &mut self.engine {
            Engine::Cpu(state) => cpu_call(STEPPING, || Ok(sample_cpu(&mut **state))),
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { state, ctx } => sample_gpu(&mut **state, ctx),
        }
    }

    /// Sets parameter `id` on the running model.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::Param`] for an unknown id or a value the descriptor refuses, [`SetupError::ReloadOnly`]
    /// for a parameter the running model takes only when it is built, and [`SetupError::Fault`] when the model panics
    /// or the device reports an error.
    pub fn set_param(&mut self, id: &str, value: impl Into<ParamValue>) -> Result<(), SetupError> {
        let value = value.into();
        let index = param_index(&self.setup.entry, id)?;
        let descriptor = &self.setup.entry.param_descriptors()[index];
        check_value(&descriptor.kind, &value).map_err(|error| param_error(id, error))?;
        let reload_only = || SetupError::ReloadOnly { id: id.to_owned() };
        // The state refuses a reload-only index itself, from the descriptors it was built with.
        let accepted = match &mut self.engine {
            Engine::Cpu(state) => catching(STEPPING, || state.set_param(index, &value))?,
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { state, ctx } => catching_on(ctx, STEPPING, || state.set_param(index, &value))?,
        };
        if accepted { Ok(()) } else { Err(reload_only()) }
    }

    /// Runs action `action_id` now, between ticks.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::UnknownAction`] for an id the model does not declare, and [`SetupError::Fault`] when the
    /// model panics or the device refuses the action's pass.
    pub fn act(&mut self, action_id: &str) -> Result<(), SetupError> {
        let index = action_index(&self.setup.entry, action_id)?;
        let refused = || Fault::refused(STEPPING, format!("the model refused its own action '{action_id}'"));
        match &mut self.engine {
            Engine::Cpu(state) => cpu_call(STEPPING, || if state.act(index) { Ok(()) } else { Err(refused()) })?,
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { state, ctx } => {
                catching_on(ctx, STEPPING, || {
                    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("henad_simulation_action"),
                    });
                    if !state.encode_action(&mut encoder, index) {
                        return Err(refused());
                    }
                    ctx.queue.submit(Some(encoder.finish()));
                    stepping::wait(ctx)
                })??;
            }
        }
        Ok(())
    }

    /// Prepares the views as a publish would, then borrows all three together. Every view is `None` for a GPU model.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the model panics while preparing its views.
    pub fn views(&mut self) -> Result<SimulationViews<'_>, Fault> {
        match &mut self.engine {
            Engine::Cpu(state) => {
                cpu_call(STEPPING, || {
                    state.prepare_view();
                    Ok(())
                })?;
                let state = &**state;
                Ok(SimulationViews {
                    grid: state.grid_view(),
                    points: state.point_view(),
                    edges: state.edge_view(),
                })
            }
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { .. } => Ok(SimulationViews {
                grid: None,
                points: None,
                edges: None,
            }),
        }
    }

    /// Relaxes a network model's layout for `budget_ms` milliseconds. Does nothing for a model without a layout.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the model panics.
    pub fn relax_layout(&mut self, budget_ms: f32) -> Result<(), Fault> {
        match &mut self.engine {
            Engine::Cpu(state) => cpu_call(STEPPING, || {
                if state.set_layout(true, budget_ms) {
                    state.relax_layout();
                    state.set_layout(false, budget_ms);
                }
                Ok(())
            }),
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { .. } => Ok(()),
        }
    }

    /// Writes the grid, points or edges as `--export` does.
    ///
    /// Every section the model has is written, so a model with a field under its agents writes both. The views are
    /// prepared first, as [`Self::views`] prepares them.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::GpuState`] for a GPU model, whose views stay on the device, [`ExportError::Io`] when
    /// `writer` fails, and [`ExportError::Fault`] when the model panics while preparing its views.
    pub fn write_state(&mut self, writer: &mut dyn Write) -> Result<(), ExportError> {
        match &mut self.engine {
            Engine::Cpu(state) => {
                // Only the prepare is scoped. The writer is not `Send`.
                cpu_call(STEPPING, || {
                    state.prepare_view();
                    Ok(())
                })?;
                write_views(&**state, writer)?;
                Ok(())
            }
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { .. } => Err(ExportError::GpuState),
        }
    }

    /// Steps to `end` under [`Fire::AfterStep`], firing each tick's actions after the step that reaches it.
    fn advance(&mut self, end: u64) -> Result<(), Fault> {
        let Self { setup, engine } = self;
        let schedule = &setup.schedule;
        match engine {
            Engine::Cpu(state) => {
                if state.tick() >= end {
                    return Ok(());
                }
                cpu_call(STEPPING, || step_cpu(&mut **state, schedule, end))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Engine::Gpu { state, ctx } => advance_gpu(&mut **state, ctx, schedule, end),
        }
    }
}

/// Steps a GPU state to `end` under [`Fire::AfterStep`], at most [`crate::gpu::MAX_STEPS_PER_SUBMISSION`] steps per
/// command buffer, and waits for the device.
#[cfg(not(target_arch = "wasm32"))]
fn advance_gpu(state: &mut dyn GpuSimState, ctx: &GpuContext, schedule: &Schedule, end: u64) -> Result<(), Fault> {
    let count = end.saturating_sub(state.tick());
    if count == 0 {
        return Ok(());
    }
    catching_on(ctx, STEPPING, || {
        let refused = stepping::run_steps_acting(state, ctx, count, schedule, Fire::AfterStep)?;
        refusal(&refused, STEPPING)
    })?
}

/// Returns the tick of the sample after `tick`: the next multiple of `interval`, or `end_tick` when that comes first.
fn next_sample_tick(tick: u64, interval: u64, end_tick: u64) -> u64 {
    (tick / interval)
        .saturating_add(1)
        .saturating_mul(interval)
        .min(end_tick)
}

/// Steps `state` to `end`, testing the schedule once per stretch between two due ticks.
fn step_cpu(state: &mut dyn SimState, schedule: &Schedule, end: u64) -> Result<(), Fault> {
    if schedule.is_empty() {
        for _ in state.tick()..end {
            state.step();
        }
        return Ok(());
    }
    while state.tick() < end {
        let stop = schedule.next_due_after(state.tick()).map_or(end, |due| due.min(end));
        for _ in state.tick()..stop {
            state.step();
        }
        refusal(&schedule.run_due(state), STEPPING)?;
    }
    Ok(())
}

/// Returns the stats of a CPU state's current tick, prepared as a publish prepares them.
fn sample_cpu(state: &mut dyn SimState) -> StatSample {
    state.prepare_view();
    StatSample {
        tick: state.tick(),
        entries: state.stats(),
    }
}

/// Returns the stats of a GPU state's current tick, blocking on a stats-only readback.
#[cfg(not(target_arch = "wasm32"))]
fn sample_gpu(state: &mut dyn GpuSimState, ctx: &GpuContext) -> Result<StatSample, Fault> {
    let entries = catching_on(ctx, STEPPING, || stepping::sample_stats(state, ctx))?;
    // Reports a fault the sample raised outside the scopes. No later call might wait.
    stepping::wait(ctx)?;
    Ok(StatSample {
        tick: state.tick(),
        entries,
    })
}

/// Writes every view of a prepared CPU state to `writer`.
fn write_views(state: &dyn SimState, mut writer: &mut dyn Write) -> io::Result<()> {
    if let Some(grid) = state.grid_view() {
        state_export::write_grid(&mut writer, grid.width, grid.height, grid.cells)?;
    }
    let points = state.point_view();
    if let Some(points) = &points {
        state_export::write_points(&mut writer, points.pos_x, points.pos_y, points.color)?;
    }
    if let (Some(points), Some(edges)) = (&points, state.edge_view()) {
        let rows = state_export::point_rows(points.pos_x, points.pos_y);
        state_export::write_edges(&mut writer, edges.src, edges.dst, edges.color.unwrap_or(&[]), &rows)?;
    }
    Ok(())
}

/// Views of one model, prepared together and borrowed from its state.
#[derive(Debug)]
#[non_exhaustive]
pub struct SimulationViews<'a> {
    pub grid: Option<GridView<'a>>,
    pub points: Option<PointView<'a>>,
    pub edges: Option<EdgeView<'a>>,
}

/// Reason [`Simulation::write_state`] wrote nothing, or stopped part way.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExportError {
    /// The model is a GPU model.
    GpuState,
    Io(io::Error),
    Fault(Fault),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GpuState => f.write_str("a GPU model's state stays on the device and cannot be exported"),
            Self::Io(_) => f.write_str("cannot write the state"),
            Self::Fault(_) => f.write_str("the model faulted while preparing its views"),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::GpuState => None,
            Self::Io(error) => Some(error),
            Self::Fault(fault) => Some(fault),
        }
    }
}

impl From<io::Error> for ExportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<Fault> for ExportError {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault)
    }
}

/// One sample of a model's stats.
#[derive(Debug, Clone)]
pub struct StatSample {
    tick: u64,
    entries: Vec<StatEntry>,
}

impl StatSample {
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Every stat the model reports, in the order it reports them.
    pub fn entries(&self) -> &[StatEntry] {
        &self.entries
    }

    /// Returns the value of stat `label`, `None` for a label the model does not report.
    pub fn get(&self, label: &str) -> Option<&StatValue> {
        self.entries
            .iter()
            .find(|entry| entry.label == label)
            .map(|entry| &entry.value)
    }

    /// Returns [`StatValue::scalar`] of stat `label`: a scalar's value, a vector's magnitude, a histogram's total
    /// count. `None` for a label the model does not report.
    pub fn scalar(&self, label: &str) -> Option<f64> {
        self.get(label).map(StatValue::scalar)
    }
}
