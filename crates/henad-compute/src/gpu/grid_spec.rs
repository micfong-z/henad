//! Declarations of a GPU grid model held as values. The engine in [`super::grid_engine`] builds its buffers,
//! pipelines and bind groups from them.
//!
//! A [`GpuGridSpec`] holds a model's buffers and passes, borrowed from its consts or owned, and its functions behind
//! [`GridHooks`]. [`GpuGridSpec::from_model`] reads a spec from a model type, and [`GpuGridSpec::new`] builds one from
//! values for a model composed at run time.
//!
//! The buffers fall into two sets. A buffer of the *dual set* has two sides, which swap once at the end of every step,
//! and a pass binds through a [`SideTable`] that says which side its reads and its writes reach. A buffer of the *plain
//! set* has one side, which every pass reads and writes in place. A model type puts every buffer in the dual set.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::metadata::Structure;
use henad_core::params::ParamValue;
use henad_core::view::{StatDescriptor, StatValue};

use crate::gpu::capacity::{Demand, PassKind, storage_bindings};
use crate::gpu::contracts::{assert_buffer_labels, assert_workgroup_size};
use crate::gpu::shader_spec::ShaderSpec;
use crate::gpu::sides::SideTable;

/// One storage buffer of a grid model's state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GridBufferSpec {
    /// Name that a shader's bindings refer to. It must not be reserved or end in `_in` or `_out`.
    pub label: Cow<'static, str>,
    /// Whether the buffer is in the dual set, with a second side that swaps with the first after every step.
    pub double_buffered: bool,
}

impl GridBufferSpec {
    /// Creates the declaration of buffer `label`.
    pub fn new(label: impl Into<Cow<'static, str>>, double_buffered: bool) -> Self {
        Self {
            label: label.into(),
            double_buffered,
        }
    }
}

/// One compute pass of a grid model, with its [`SideTable`] and its cost.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GridPassSpec {
    /// Name of the pass, part of its GPU labels.
    pub label: Cow<'static, str>,
    /// Shader of the pass, which declares `@workgroup_size(N, N)` for [`GpuGridSpec::workgroup_size`].
    pub shader: ShaderSpec,
    /// Sides the pass binds.
    pub sides: SideTable,
    /// Cost of one invocation in work units, as a graph model's checker counts it. Zero when the cost is unknown, as
    /// for a hand-written shader, and [`Self::new`] leaves it at zero.
    pub cost: u64,
    /// Size in bytes of the uniform block the shader binds. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub uniform_bytes: u64,
    /// Bytes of the shader's `var<workgroup>` declarations. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub workgroup_bytes: u64,
}

impl GridPassSpec {
    /// Creates pass `label`.
    pub fn new(label: impl Into<Cow<'static, str>>, shader: ShaderSpec, sides: SideTable) -> Self {
        Self {
            label: label.into(),
            shader,
            sides,
            cost: 0,
            uniform_bytes: 0,
            workgroup_bytes: 0,
        }
    }
}

/// A one-off action the user can trigger: a rerun on the host, then passes dispatched over the step domain.
///
/// A press calls [`GridHooks::rerun`] first, uploads the buffers it returns to their current sides, then dispatches
/// [`Self::passes`] in order, each in place.
#[derive(Debug)]
#[non_exhaustive]
pub struct GridActionSpec {
    /// Id of the action.
    pub id: Cow<'static, str>,
    /// Passes of the action, in the order they run. A pass's label is part of the labels of its pass and its uniform.
    pub passes: Vec<GridPassSpec>,
    /// Cost per cell in work units of the rerun on the host, zero for an action with none. [`Self::new`] leaves it at
    /// zero.
    pub rerun_cost: u64,
}

impl GridActionSpec {
    /// Creates the action `id`, which runs `shader` in place in a pass labelled `id`, over `buffers` buffers.
    pub fn new(id: impl Into<Cow<'static, str>>, shader: ShaderSpec, buffers: usize) -> Self {
        let id = id.into();
        let pass = GridPassSpec::new(id.clone(), shader, SideTable::in_place(buffers));
        Self::with_passes(id, vec![pass])
    }

    /// Creates the action `id`, which runs `passes` in order. Each pass binds through its own table, usually
    /// [`SideTable::in_place`], since nothing swaps after an action and a write to the next side would be lost.
    pub fn with_passes(id: impl Into<Cow<'static, str>>, passes: Vec<GridPassSpec>) -> Self {
        Self {
            id: id.into(),
            passes,
            rerun_cost: 0,
        }
    }
}

/// Pass whose uniform block [`GridHooks::pass_params_bytes`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridPassId {
    /// Entry of [`GpuGridSpec::steps`] at this index.
    Step(usize),
    /// The [`GpuGridSpec::display`] pass.
    Display,
    /// The [`GpuGridSpec::reduce`] pass.
    Reduce,
    /// Pass `pass` of the entry of [`GpuGridSpec::actions`] at index `action`.
    Action {
        /// Index of the action.
        action: usize,
        /// Index of the pass within the action.
        pass: usize,
    },
}

/// A press of an action: the word the press draws from and the number of presses so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GridPress {
    /// Seed of the press, advanced on every press of any action so that pressing twice draws twice.
    pub seed: u32,
    /// Number of presses of any action so far, this one included. Zero before the first press.
    pub count: u32,
}

impl GridPress {
    /// Creates the press `count` with seed `seed`.
    pub fn new(seed: u32, count: u32) -> Self {
        Self { seed, count }
    }
}

/// Values a uniform block or a rerun reads besides the parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GridPassCtx {
    /// Width of the grid, at least 1.
    pub width: u32,
    /// Height of the grid, at least 1.
    pub height: u32,
    /// Seed of the run, `None` for the model's fixed default seed.
    pub seed: Option<u64>,
    /// Latest press, the press being run in an action, and the press before the first, with a count of zero, otherwise.
    pub press: GridPress,
    /// Steps done when the hook runs: zero at construction, and for the stat values the steps done when the reduce
    /// pass was recorded.
    pub tick: u64,
}

/// Buffers that a rerun on the host wrote, each as its buffer's position in [`GpuGridSpec::buffers`] and its new
/// words.
pub type RerunUploads = Vec<(usize, Vec<u32>)>;

/// Functions of a GPU grid model that the engine calls, each the counterpart of the [`GpuGridModel`] function of the
/// same name.
///
/// The engine calls no hook per step or per element. It reads the sizes and seeds once at construction, a uniform block
/// once per pass at construction and once per action pass per press, a rerun once per press, and the stat values once
/// per read.
pub trait GridHooks: Send + Sync {
    /// Returns the grid size for `params`, before the engine clamps each side to at least 1.
    fn dims(&self, params: &[ParamValue]) -> (u32, u32);

    /// Returns the length in `u32` words of each buffer, in [`GpuGridSpec::buffers`] order.
    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize>;

    /// Returns the dispatch domain of every step pass and every action pass, in invocations.
    fn step_dims(&self, width: u32, height: u32) -> (u32, u32);

    /// Returns the initial contents of each buffer, one vector per buffer.
    fn seed_buffers(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>>;

    /// Returns the uniform block of `pass` as raw bytes.
    ///
    /// A model type's block depends on the parameters and, in an action, the press seed. A grid rule's holds a word of
    /// the run's seed.
    fn pass_params_bytes(&self, pass: GridPassId, ctx: GridPassCtx, params: &[ParamValue]) -> Vec<u8>;

    /// Runs the host part of action `action` for the press in `ctx`, and returns the buffers it writes.
    ///
    /// `stream` is the action stream, which starts at [`action_seed`](henad_core::action::action_seed) of the run's
    /// seed and continues across presses. The default runs nothing.
    ///
    /// # Errors
    ///
    /// Returns a message for a rerun that faults. The engine reports it as a refused fault and runs none of the
    /// action's passes.
    fn rerun(
        &self,
        _action: usize,
        _ctx: GridPassCtx,
        _params: &[ParamValue],
        _stream: &mut u64,
    ) -> Result<RerunUploads, String> {
        Ok(Vec::new())
    }

    /// Turns the counters read back from the reduce shader into values, in [`GpuGridSpec::stats`] order.
    ///
    /// `counts` holds [`GpuGridSpec::counters`] words. `ctx` holds the tick of the reduce, and `params` the values the
    /// model was built with.
    fn stats(&self, counts: &[u32], ctx: GridPassCtx, params: &[ParamValue]) -> Vec<StatValue>;
}

/// Hooks of model `M`, each calling the associated function of the same name.
struct ModelHooks<M>(PhantomData<M>);

impl<M: GpuGridModel> GridHooks for ModelHooks<M> {
    fn dims(&self, params: &[ParamValue]) -> (u32, u32) {
        M::dims(params)
    }

    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize> {
        M::buffer_lens(width, height)
    }

    fn step_dims(&self, width: u32, height: u32) -> (u32, u32) {
        M::step_dims(width, height)
    }

    fn seed_buffers(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        M::seed_buffers(width, height, params, seed)
    }

    /// Returns the step block for every pass but an action, which shares it, and the action's block for an action.
    fn pass_params_bytes(&self, pass: GridPassId, ctx: GridPassCtx, params: &[ParamValue]) -> Vec<u8> {
        match pass {
            GridPassId::Action { action, .. } => {
                M::action_params_bytes(action, ctx.width, ctx.height, params, ctx.press.seed)
            }
            GridPassId::Step(_) | GridPassId::Display | GridPassId::Reduce => {
                M::step_params_bytes(ctx.width, ctx.height, params)
            }
        }
    }

    fn stats(&self, counts: &[u32], _ctx: GridPassCtx, _params: &[ParamValue]) -> Vec<StatValue> {
        M::stats(counts)
    }
}

/// Declarations of a GPU grid model, held as values.
///
/// Each label the engine gives a wgpu object starts with [`Self::id`].
#[non_exhaustive]
pub struct GpuGridSpec {
    /// Id of the model.
    pub id: Cow<'static, str>,
    /// Side `N` of the `@workgroup_size(N, N)` every shader declares, actions included.
    pub workgroup_size: u32,
    /// Storage buffers of the model's state. A binding names one of them, with an optional `_in` or `_out`.
    pub buffers: Vec<GridBufferSpec>,
    /// Passes of one step, run in order over the step domain of [`Self::hooks`].
    pub steps: Vec<GridPassSpec>,
    /// Display pass, dispatched once per display texel.
    pub display: GridPassSpec,
    /// Reduce pass, dispatched once per cell.
    pub reduce: GridPassSpec,
    /// One-off actions the user can trigger, in the order of their buttons. [`Self::new`] leaves this empty.
    pub actions: Vec<GridActionSpec>,
    /// Stat series, which [`GridHooks::stats`] fills from the reduce shader's counters.
    pub stats: Cow<'static, [StatDescriptor]>,
    /// Number of `atomic<u32>` counters the reduce shader's `counters` binding holds, at least 1. [`Self::new`] sets
    /// one per stat series, as a model type's reduce shader counts.
    pub counters: usize,
    /// Cost per cell in work units of what the host runs at build, the seeds included. Zero when unknown, as for a
    /// model type, and [`Self::new`] leaves it at zero.
    pub init_cost: u64,
    /// Functions the engine calls for the sizes, the seeds, the uniform blocks, the reruns and the stat values.
    pub hooks: Box<dyn GridHooks>,
}

/// Prints the declarations, and leaves out the hooks.
impl fmt::Debug for GpuGridSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuGridSpec")
            .field("id", &self.id)
            .field("workgroup_size", &self.workgroup_size)
            .field("buffers", &self.buffers)
            .field("steps", &self.steps)
            .field("display", &self.display)
            .field("reduce", &self.reduce)
            .field("actions", &self.actions)
            .field("stats", &self.stats)
            .field("counters", &self.counters)
            .field("init_cost", &self.init_cost)
            .finish_non_exhaustive()
    }
}

impl GpuGridSpec {
    /// Creates the spec of model `id` with no actions.
    ///
    /// [`Self::actions`], [`Self::counters`] and [`Self::init_cost`] are public fields, for a model that declares
    /// otherwise.
    #[expect(
        clippy::too_many_arguments,
        reason = "every argument is a declaration the engine cannot run without"
    )]
    pub fn new(
        id: impl Into<Cow<'static, str>>,
        workgroup_size: u32,
        buffers: Vec<GridBufferSpec>,
        steps: Vec<GridPassSpec>,
        display: GridPassSpec,
        reduce: GridPassSpec,
        stats: impl Into<Cow<'static, [StatDescriptor]>>,
        hooks: Box<dyn GridHooks>,
    ) -> Self {
        let stats = stats.into();
        Self {
            id: id.into(),
            workgroup_size,
            buffers,
            steps,
            display,
            reduce,
            actions: Vec::new(),
            counters: stats.len().max(1),
            stats,
            init_cost: 0,
            hooks,
        }
    }

    /// Returns the spec of a model with one step pass, every buffer in the dual set, and the single orientation of a
    /// model type: every pass but an action reads each current side and writes each next side.
    ///
    /// `buffers` are the labels of the buffers, and the passes take the labels `step`, `display` and `reduce`.
    #[expect(
        clippy::too_many_arguments,
        reason = "every argument is a declaration the engine cannot run without"
    )]
    pub fn single_step(
        id: impl Into<Cow<'static, str>>,
        workgroup_size: u32,
        buffers: Vec<Cow<'static, str>>,
        step: ShaderSpec,
        display: ShaderSpec,
        reduce: ShaderSpec,
        stats: impl Into<Cow<'static, [StatDescriptor]>>,
        hooks: Box<dyn GridHooks>,
    ) -> Self {
        let count = buffers.len();
        let swapping = |label: &'static str, shader| GridPassSpec::new(label, shader, SideTable::swapping(count));
        Self::new(
            id,
            workgroup_size,
            buffers
                .into_iter()
                .map(|label| GridBufferSpec::new(label, true))
                .collect(),
            vec![swapping("step", step)],
            swapping("display", display),
            swapping("reduce", reduce),
            stats,
            hooks,
        )
    }

    /// Returns the declarations of model `M`, borrowing its shaders, its binding names and its stats.
    ///
    /// Every buffer is in the dual set. The step, display and reduce passes read each current side and write each next
    /// side, and every action binds in place. One swap per step then gives each pass the orientation the model's
    /// shaders assume.
    pub(crate) fn from_model<M: GpuGridModel>() -> Self {
        let mut spec = Self::single_step(
            M::ID,
            M::WORKGROUP_SIZE,
            M::BUFFERS.iter().map(|label| Cow::Borrowed(*label)).collect(),
            ShaderSpec::borrowed(M::STEP_SHADER, M::STEP_BINDINGS),
            ShaderSpec::borrowed(M::DISPLAY_SHADER, M::DISPLAY_BINDINGS),
            ShaderSpec::borrowed(M::REDUCE_SHADER, M::REDUCE_BINDINGS),
            M::STATS,
            Box::new(ModelHooks::<M>(PhantomData)),
        );
        spec.actions = M::ACTIONS
            .iter()
            .map(|action| {
                GridActionSpec::new(
                    action.desc.id(),
                    ShaderSpec::borrowed(action.shader, action.bindings),
                    M::BUFFERS.len(),
                )
            })
            .collect();
        spec
    }

    /// Returns the [`Structure`] an entry of this model declares.
    pub fn structure(&self) -> Structure {
        Structure::GpuGrid {
            buffers: Cow::Owned(self.buffers.iter().map(|buffer| buffer.label.clone()).collect()),
            workgroup: self.workgroup_size,
        }
    }

    /// Returns the grid size for `params`, each side clamped to at least 1.
    pub fn dims(&self, params: &[ParamValue]) -> (u32, u32) {
        let (width, height) = self.hooks.dims(params);
        (width.max(1), height.max(1))
    }

    /// Resources that would be allocated for this model based on `params`, with the work of every pass and of the
    /// host's runs.
    pub fn demand(&self, params: &[ParamValue], limits: &wgpu::Limits) -> Demand {
        let (width, height) = self.dims(params);

        let mut demand = Demand::default();
        for ((k, len), buffer) in self
            .hooks
            .buffer_lens(width, height)
            .into_iter()
            .enumerate()
            .zip(&self.buffers)
        {
            demand.push_sides(&format!("{}_buffer{k}", self.id), len, buffer.double_buffered);
        }
        demand.set_display(width, height, limits);
        let texels = demand.texture.map_or(0, |(w, h)| u64::from(w) * u64::from(h));
        let (step_x, step_y) = self.hooks.step_dims(width, height);
        let invocations = Invocations {
            step: u64::from(step_x) * u64::from(step_y),
            texels,
            cells: u64::from(width) * u64::from(height),
        };
        for pass in self.declared_passes() {
            let recorded = demand.push_pass(
                pass.label,
                pass.kind,
                pass.storage,
                pass.cost.saturating_mul(invocations.of(pass.kind)),
            );
            recorded.uniform_bytes = pass.uniform_bytes;
            recorded.workgroup_bytes = pass.workgroup_bytes;
        }
        let cells = invocations.cells;
        if self.init_cost > 0 {
            demand.push_host(format!("{}_init", self.id), self.init_cost.saturating_mul(cells));
        }
        for action in self.actions.iter().filter(|action| action.rerun_cost > 0) {
            demand.push_host(
                format!("{}_action_{}_rerun", self.id, action.id),
                action.rerun_cost.saturating_mul(cells),
            );
        }
        demand
    }

    /// Every pass with its kind, the storage buffers it binds, its cost per invocation and its memory sizes. Read by
    /// both [`Self::demand`] and [`Self::max_storage_bindings`], so the device that a host requests and the shortfall
    /// that the UI reports cannot disagree.
    fn declared_passes(&self) -> Vec<DeclaredPass> {
        let declared = |pass: &GridPassSpec, kind: PassKind, label: String| DeclaredPass {
            label,
            kind,
            storage: storage_bindings(&pass.shader.bindings),
            cost: pass.cost,
            uniform_bytes: pass.uniform_bytes,
            workgroup_bytes: pass.workgroup_bytes,
        };
        let mut passes: Vec<DeclaredPass> = self
            .steps
            .iter()
            .map(|pass| declared(pass, PassKind::Step, format!("{}_{}", self.id, pass.label)))
            .collect();
        passes.push(declared(
            &self.display,
            PassKind::Display,
            format!("{}_{}", self.id, self.display.label),
        ));
        passes.push(declared(
            &self.reduce,
            PassKind::Reduce,
            format!("{}_{}", self.id, self.reduce.label),
        ));
        for (index, action) in self.actions.iter().enumerate() {
            passes.extend(action.passes.iter().map(|pass| {
                declared(
                    pass,
                    PassKind::Action(index),
                    format!("{}_action_{}", self.id, pass.label),
                )
            }));
        }
        passes
    }

    /// Returns the most storage buffers any pass binds.
    pub fn max_storage_bindings(&self) -> u32 {
        self.declared_passes()
            .into_iter()
            .map(|pass| pass.storage)
            .max()
            .unwrap_or(0)
    }

    /// Returns each pass the engine builds, as its name in a message, its shader and its side table.
    pub(crate) fn passes(&self) -> impl Iterator<Item = (&str, &GridPassSpec)> {
        self.steps
            .iter()
            .chain([&self.display, &self.reduce])
            .chain(self.actions.iter().flat_map(|action| &action.passes))
            .map(|pass| (pass.label.as_ref(), pass))
    }

    /// Asserts that the buffer labels, every shader's workgroup size and every side table agree with what the engine
    /// binds and dispatches.
    ///
    /// # Panics
    ///
    /// Panics if a buffer label is reserved or ends in `_in` or `_out`, if a shader declares a workgroup size other
    /// than [`Self::workgroup_size`] on both sides, or if a side table holds another number of rows than there are
    /// buffers.
    pub(crate) fn assert_contracts(&self) {
        assert_buffer_labels(&self.id, self.buffers.iter().map(|buffer| buffer.label.as_ref()));
        // Every pass dispatches square workgroups of `workgroup_size`.
        let square = [self.workgroup_size, self.workgroup_size, 1];
        for (label, pass) in self.passes() {
            assert_workgroup_size(&self.id, label, &pass.shader.source, square);
            assert!(
                pass.sides.rows().len() == self.buffers.len(),
                "{}: the side table of {label} holds {} rows for {} buffers",
                self.id,
                pass.sides.rows().len(),
                self.buffers.len()
            );
        }
    }
}

/// A pass as [`GpuGridSpec::demand`] counts it.
struct DeclaredPass {
    label: String,
    kind: PassKind,
    storage: u32,
    cost: u64,
    uniform_bytes: u64,
    workgroup_bytes: u64,
}

/// Invocations of each kind of pass for one grid size.
struct Invocations {
    /// Invocations of a step or action pass.
    step: u64,
    /// Texels of the display texture.
    texels: u64,
    /// Cells of the grid.
    cells: u64,
}

impl Invocations {
    /// Returns the invocations of a pass of kind `kind`.
    fn of(&self, kind: PassKind) -> u64 {
        match kind {
            PassKind::Step | PassKind::Action(_) => self.step,
            PassKind::Display => self.texels,
            PassKind::Reduce => self.cells,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use henad_core::action::ActionDescriptor;
    use henad_core::authoring::model::binding::{BindingDecl, BindingKind};
    use henad_core::authoring::model::gpu_grid_model::{GpuGridAction, GpuGridModel};
    use henad_core::helpers::u32_param;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::view::{StatDescriptor, StatValue};

    use super::{GpuGridSpec, GridPassCtx, GridPassId, GridPress, ShaderSpec};
    use crate::gpu::capacity::PassKind;
    use crate::gpu::sides::{BufferSide, SideRow, SideTable};

    const fn storage(name: &'static str, read_only: bool) -> BindingDecl {
        BindingDecl {
            name,
            kind: BindingKind::Storage { read_only },
        }
    }

    const fn uniform(name: &'static str) -> BindingDecl {
        BindingDecl {
            name,
            kind: BindingKind::Uniform,
        }
    }

    /// A model that is never built, with a distinct value in every declaration and every function.
    struct Probe;

    impl GpuGridModel for Probe {
        const NAME: &'static str = "Probe";
        const ID: &'static str = "probe";
        const DESCRIPTION: &'static str = "Declarations read by the spec test";
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255]];
        const WORKGROUP_SIZE: u32 = 8;
        const STATS: &'static [StatDescriptor] = &[
            StatDescriptor::new("Low", [1, 2, 3, 255]),
            StatDescriptor::new("High", [4, 5, 6, 255]),
        ];
        const ACTIONS: &'static [GpuGridAction] = &[GpuGridAction {
            desc: ActionDescriptor::new("spark", "Spark"),
            shader: "action shader",
            bindings: &[storage("state", false), uniform("params")],
        }];
        const BUFFERS: &'static [&'static str] = &["state", "rng"];
        const STEP_BINDINGS: &'static [BindingDecl] = &[
            storage("state_in", true),
            storage("state_out", false),
            storage("rng_in", true),
            storage("rng_out", false),
            uniform("params"),
        ];
        const DISPLAY_BINDINGS: &'static [BindingDecl] = &[storage("state", true), uniform("dims")];
        const REDUCE_BINDINGS: &'static [BindingDecl] =
            &[storage("state", true), storage("counters", false), uniform("dims")];
        const STEP_SHADER: &'static str = "step shader";
        const DISPLAY_SHADER: &'static str = "display shader";
        const REDUCE_SHADER: &'static str = "reduce shader";

        fn param_descriptors() -> Vec<ParamDescriptor> {
            vec![
                u32_param("grid_width", "Grid Width", 12, 0, 64),
                u32_param("grid_height", "Grid Height", 10, 0, 64),
            ]
        }

        fn dims(params: &[ParamValue]) -> (u32, u32) {
            match params {
                [ParamValue::U32(width), ParamValue::U32(height), ..] => (*width, *height),
                _ => (12, 10),
            }
        }

        fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
            vec![(width * height) as usize, 3]
        }

        fn step_dims(width: u32, height: u32) -> (u32, u32) {
            (width.div_ceil(4), height)
        }

        fn seed_buffers(width: u32, _height: u32, _params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
            vec![vec![width], vec![seed.map_or(0, |seed| seed as u32)]]
        }

        fn step_params_bytes(width: u32, height: u32, _params: &[ParamValue]) -> Vec<u8> {
            vec![width as u8, height as u8]
        }

        fn action_params_bytes(action: usize, _width: u32, _height: u32, _params: &[ParamValue], seed: u32) -> Vec<u8> {
            vec![action as u8, seed as u8]
        }

        fn stats(counts: &[u32]) -> Vec<StatValue> {
            counts
                .iter()
                .map(|&count| StatValue::Scalar(f64::from(count) * 2.0))
                .collect()
        }
    }

    /// Returns whether `shader` borrows `source` and the name of each of `bindings`, in the kinds they declare.
    fn borrows(shader: &ShaderSpec, source: &str, bindings: &[BindingDecl]) -> bool {
        matches!(shader.source, Cow::Borrowed(text) if text == source)
            && shader.bindings.len() == bindings.len()
            && shader.bindings.iter().zip(bindings).all(|(binding, decl)| {
                matches!(binding.name, Cow::Borrowed(name) if name == decl.name) && binding.kind == decl.kind
            })
    }

    /// The spec of a model borrows every declaration it holds, and each hook calls the function of the same name.
    #[test]
    fn a_model_spec_borrows_its_declarations_and_calls_its_functions() {
        let spec = GpuGridSpec::from_model::<Probe>();
        assert!(matches!(spec.id, Cow::Borrowed("probe")), "id");
        assert_eq!(spec.workgroup_size, 8);
        let labels: Vec<&str> = spec.buffers.iter().map(|buffer| buffer.label.as_ref()).collect();
        assert_eq!(labels, ["state", "rng"]);
        assert!(
            spec.buffers.iter().all(|buffer| buffer.double_buffered),
            "every buffer is in the dual set"
        );
        assert_eq!(spec.steps.len(), 1);
        assert!(
            borrows(&spec.steps[0].shader, Probe::STEP_SHADER, Probe::STEP_BINDINGS),
            "step"
        );
        assert!(
            borrows(&spec.display.shader, Probe::DISPLAY_SHADER, Probe::DISPLAY_BINDINGS),
            "display"
        );
        assert!(
            borrows(&spec.reduce.shader, Probe::REDUCE_SHADER, Probe::REDUCE_BINDINGS),
            "reduce"
        );
        assert_eq!(spec.actions.len(), 1);
        let action = &spec.actions[0];
        assert!(matches!(action.id, Cow::Borrowed("spark")), "action id");
        assert_eq!(action.passes.len(), 1);
        assert!(
            borrows(
                &action.passes[0].shader,
                Probe::ACTIONS[0].shader,
                Probe::ACTIONS[0].bindings
            ),
            "action"
        );
        assert_eq!(
            (spec.init_cost, action.rerun_cost, spec.steps[0].cost),
            (0, 0, 0),
            "a model type's cost is unknown"
        );
        assert!(
            matches!(spec.stats, Cow::Borrowed(stats) if stats == Probe::STATS),
            "stats"
        );

        let params = [ParamValue::U32(12), ParamValue::U32(10)];
        assert_eq!(spec.hooks.dims(&params), (12, 10));
        assert_eq!(spec.hooks.buffer_lens(12, 10), vec![120, 3]);
        assert_eq!(spec.hooks.step_dims(12, 10), (3, 10));
        assert_eq!(
            spec.hooks.seed_buffers(12, 10, &params, Some(7)),
            vec![vec![12], vec![7]]
        );
        let ctx = GridPassCtx {
            width: 12,
            height: 10,
            seed: Some(7),
            press: GridPress::new(9, 1),
            tick: 0,
        };
        for pass in [GridPassId::Step(0), GridPassId::Display, GridPassId::Reduce] {
            assert_eq!(
                spec.hooks.pass_params_bytes(pass, ctx, &params),
                vec![12, 10],
                "{pass:?}"
            );
        }
        assert_eq!(
            spec.hooks
                .pass_params_bytes(GridPassId::Action { action: 0, pass: 0 }, ctx, &params),
            vec![0, 9]
        );
        let mut stream = 3;
        assert_eq!(spec.hooks.rerun(0, ctx, &params, &mut stream), Ok(Vec::new()));
        assert_eq!(stream, 3, "a model type reruns nothing");
        let values: Vec<f64> = spec
            .hooks
            .stats(&[3, 5], ctx, &params)
            .iter()
            .map(|value| match value {
                StatValue::Scalar(value) => *value,
                other => panic!("the probe reports scalars, got {other:?}"),
            })
            .collect();
        assert_eq!(values, [6.0, 10.0]);
    }

    /// The demand names every buffer side and every pass after the model, and counts each pass's storage bindings.
    #[test]
    fn a_spec_demand_names_each_buffer_and_pass() {
        let spec = GpuGridSpec::from_model::<Probe>();
        // A zero side is clamped to 1, as the engine builds it.
        let demand = spec.demand(&[ParamValue::U32(0), ParamValue::U32(10)], &wgpu::Limits::default());
        let buffers: Vec<(&str, u64)> = demand
            .buffers
            .iter()
            .map(|alloc| (alloc.label.as_str(), alloc.bytes))
            .collect();
        assert_eq!(
            buffers,
            [
                ("probe_buffer0_a", 40),
                ("probe_buffer0_b", 40),
                ("probe_buffer1_a", 12),
                ("probe_buffer1_b", 12),
            ]
        );
        assert_eq!(demand.texture, Some((1, 10)));
        let passes: Vec<(&str, PassKind, u32, u64)> = demand
            .passes
            .iter()
            .map(|pass| (pass.label.as_str(), pass.kind, pass.storage, pass.work))
            .collect();
        assert_eq!(
            passes,
            [
                ("probe_step", PassKind::Step, 4, 0),
                ("probe_display", PassKind::Display, 1, 0),
                ("probe_reduce", PassKind::Reduce, 2, 0),
                ("probe_action_spark", PassKind::Action(0), 1, 0),
            ]
        );
        assert!(demand.host.is_empty(), "a model type runs nothing on the host");
        assert_eq!(spec.max_storage_bindings(), 4);
    }

    #[test]
    #[should_panic(expected = "probe: the spark shader declares @workgroup_size(16, 16, 1)")]
    fn an_action_shader_of_another_workgroup_size_is_refused() {
        let mut spec = GpuGridSpec::from_model::<Probe>();
        spec.actions[0].passes[0].shader.source =
            Cow::Owned("@compute @workgroup_size(16, 16) fn main() {}".to_owned());
        spec.assert_contracts();
    }

    /// Returns a spec of [`Probe`] whose shaders declare the workgroup size the engine dispatches.
    fn valid_spec() -> GpuGridSpec {
        let mut spec = GpuGridSpec::from_model::<Probe>();
        let square = || Cow::Owned("@compute @workgroup_size(8, 8) fn main() {}".to_owned());
        spec.steps[0].shader.source = square();
        spec.display.shader.source = square();
        spec.reduce.shader.source = square();
        spec.actions[0].passes[0].shader.source = square();
        spec
    }

    #[test]
    fn a_valid_spec_meets_its_contracts() {
        valid_spec().assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: the side table of step holds 1 rows for 2 buffers")]
    fn a_side_table_of_another_length_is_refused() {
        let mut spec = valid_spec();
        spec.steps[0].sides = SideTable::swapping(1);
        spec.assert_contracts();
    }

    /// The step, display and reduce passes read the current side and write the next, and an action stays in place.
    #[test]
    fn a_model_spec_binds_in_the_single_orientation() {
        let spec = GpuGridSpec::from_model::<Probe>();
        let swapping = SideRow::new(BufferSide::Current, BufferSide::Next);
        let in_place = SideRow::new(BufferSide::Current, BufferSide::Current);
        for pass in [&spec.steps[0], &spec.display, &spec.reduce] {
            assert_eq!(pass.sides.rows(), [swapping, swapping], "{}", pass.label);
        }
        assert_eq!(spec.actions[0].passes[0].sides.rows(), [in_place, in_place]);
    }

    /// A pass within the per-element limit of a graph model is refused over a large grid, and so is an init the host
    /// could not run in time, before anything is allocated.
    #[test]
    fn work_past_the_budgets_is_a_shortfall() {
        let mut spec = GpuGridSpec::from_model::<Probe>();
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: u64::MAX,
            max_buffer_size: u64::MAX,
            ..wgpu::Limits::default()
        };
        let small = [ParamValue::U32(64), ParamValue::U32(64)];
        let large = [ParamValue::U32(16_384), ParamValue::U32(16_384)];
        spec.steps[0].cost = 65_536;
        spec.init_cost = 65_536;
        assert!(spec.demand(&small, &limits).shortfalls(&limits).is_empty());
        let found = spec.demand(&large, &limits).shortfalls(&limits);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("'probe_step' is the heaviest"), "{}", found[0]);
        assert!(found[1].contains("'probe_init'"), "{}", found[1]);
    }

    /// A plain buffer has one side, a pass's work is its cost times its invocations, and the host's runs are counted
    /// per cell.
    #[test]
    fn a_spec_demand_counts_plain_buffers_and_work() {
        let mut spec = GpuGridSpec::from_model::<Probe>();
        spec.buffers[1].double_buffered = false;
        spec.steps[0].cost = 3;
        spec.display.cost = 5;
        spec.reduce.cost = 7;
        spec.actions[0].passes[0].cost = 11;
        spec.actions[0].rerun_cost = 13;
        spec.init_cost = 17;
        let demand = spec.demand(&[ParamValue::U32(12), ParamValue::U32(10)], &wgpu::Limits::default());
        let buffers: Vec<&str> = demand.buffers.iter().map(|alloc| alloc.label.as_str()).collect();
        assert_eq!(buffers, ["probe_buffer0_a", "probe_buffer0_b", "probe_buffer1_a"]);
        let work: Vec<u64> = demand.passes.iter().map(|pass| pass.work).collect();
        // The probe's step domain is 3 by 10, and its grid and texture 12 by 10.
        assert_eq!(work, [3 * 30, 5 * 120, 7 * 120, 11 * 30]);
        let host: Vec<(&str, u64)> = demand.host.iter().map(|run| (run.label.as_str(), run.work)).collect();
        assert_eq!(host, [("probe_init", 17 * 120), ("probe_action_spark_rerun", 13 * 120)]);
    }

    /// The demand records each pass's uniform block and workgroup memory, and refuses a reduce pass whose workgroup
    /// memory is past the baseline's 16 KiB before anything is allocated.
    #[test]
    fn a_spec_demand_counts_pass_memory() {
        let mut spec = GpuGridSpec::from_model::<Probe>();
        spec.steps[0].uniform_bytes = 32;
        spec.reduce.workgroup_bytes = 16_384;
        let params = [ParamValue::U32(12), ParamValue::U32(10)];
        let limits = wgpu::Limits::default();
        let demand = spec.demand(&params, &limits);
        let memory: Vec<(u64, u64)> = demand
            .passes
            .iter()
            .map(|pass| (pass.uniform_bytes, pass.workgroup_bytes))
            .collect();
        assert_eq!(memory, [(32, 0), (0, 0), (0, 16_384), (0, 0)]);
        assert!(demand.shortfalls(&limits).is_empty());
        spec.reduce.workgroup_bytes = 16_388;
        let found = spec.demand(&params, &limits).shortfalls(&limits);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'probe_reduce'"), "{}", found[0]);
    }
}
