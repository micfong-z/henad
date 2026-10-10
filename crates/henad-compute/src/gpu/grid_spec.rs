//! Declarations of a GPU grid model held as values. The engine in [`super::grid_engine`] builds its buffers,
//! pipelines and bind groups from them.
//!
//! A [`GpuGridSpec`] holds a model's shaders and binding lists, borrowed from its consts or owned, and its functions
//! behind [`GridHooks`]. [`GpuGridSpec::from_model`] reads a spec from a model type.

use std::borrow::Cow;
use std::marker::PhantomData;

use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::params::ParamValue;
use henad_core::view::{StatDescriptor, StatValue};

use crate::gpu::capacity::{Demand, storage_bindings};
use crate::gpu::contracts::{assert_buffer_labels, assert_workgroup_size};

/// A compute shader's WGSL source, with its `@group(0)` declarations in `@binding` order.
pub(crate) struct GridShaderSpec {
    /// WGSL source, with `main` as the entry point.
    pub(crate) source: Cow<'static, str>,
    /// Declarations of the shader's bindings.
    pub(crate) bindings: Cow<'static, [BindingDecl]>,
}

impl GridShaderSpec {
    /// Returns a shader whose source and declarations are a model's consts.
    fn borrowed(source: &'static str, bindings: &'static [BindingDecl]) -> Self {
        Self {
            source: Cow::Borrowed(source),
            bindings: Cow::Borrowed(bindings),
        }
    }
}

/// A one-off pass the user can trigger, dispatched over the step pass's domain.
pub(crate) struct GridActionSpec {
    /// Id of the action, part of the labels of its pass and its uniform.
    pub(crate) id: Cow<'static, str>,
    /// Shader of the pass.
    pub(crate) shader: GridShaderSpec,
}

/// Functions of a GPU grid model that the engine calls, each the counterpart of the [`GpuGridModel`] function of the
/// same name.
pub(crate) trait GridHooks: Send + Sync {
    /// Returns the grid size for `params`, before the engine clamps each side to at least 1.
    fn dims(&self, params: &[ParamValue]) -> (u32, u32);

    /// Returns the length in `u32` words of each ping-ponged buffer, in [`GpuGridSpec::buffers`] order.
    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize>;

    /// Returns the dispatch domain of the step pass and of every action, in invocations.
    fn step_dims(&self, width: u32, height: u32) -> (u32, u32);

    /// Returns the initial contents of each ping-ponged buffer, one vector per label.
    fn seed_buffers(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>>;

    /// Returns the step shader's uniform block as raw bytes, for a run seeded with `seed`.
    ///
    /// A model type's block depends on the parameters alone, and a grid rule's holds a word of the seed.
    fn step_params_bytes(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<u8>;

    /// Returns the uniform block of action `action` as raw bytes, for a press drawn from `seed`.
    fn action_params_bytes(&self, action: usize, width: u32, height: u32, params: &[ParamValue], seed: u32) -> Vec<u8>;

    /// Turns the counters read back from the reduce shader into values, in [`GpuGridSpec::stats`] order.
    fn stats(&self, counts: &[u32]) -> Vec<StatValue>;
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

    fn step_params_bytes(&self, width: u32, height: u32, params: &[ParamValue], _seed: Option<u64>) -> Vec<u8> {
        M::step_params_bytes(width, height, params)
    }

    fn action_params_bytes(&self, action: usize, width: u32, height: u32, params: &[ParamValue], seed: u32) -> Vec<u8> {
        M::action_params_bytes(action, width, height, params, seed)
    }

    fn stats(&self, counts: &[u32]) -> Vec<StatValue> {
        M::stats(counts)
    }
}

/// Declarations of a GPU grid model, held as values.
///
/// Each label the engine gives a wgpu object starts with [`Self::id`].
pub(crate) struct GpuGridSpec {
    /// Id of the model.
    pub(crate) id: Cow<'static, str>,
    /// Side `N` of the `@workgroup_size(N, N)` every shader declares, actions included.
    pub(crate) workgroup_size: u32,
    /// Labels of the buffers ping-ponged per step. A binding names one of them, with an optional `_in` or `_out`.
    pub(crate) buffers: &'static [&'static str],
    /// Step shader, dispatched over the step domain of [`Self::hooks`].
    pub(crate) step: GridShaderSpec,
    /// Display shader, dispatched once per display texel.
    pub(crate) display: GridShaderSpec,
    /// Reduce shader, dispatched once per cell.
    pub(crate) reduce: GridShaderSpec,
    /// One-off passes the user can trigger, in the order of their buttons.
    pub(crate) actions: Vec<GridActionSpec>,
    /// Stat series, one per counter of the reduce shader's `counters` binding.
    pub(crate) stats: Cow<'static, [StatDescriptor]>,
    /// Functions the engine calls for the sizes, the seeds, the uniform blocks and the stat values.
    pub(crate) hooks: Box<dyn GridHooks>,
}

impl GpuGridSpec {
    /// Returns the declarations of model `M`, borrowing its shaders, its binding lists and its stats.
    pub(crate) fn from_model<M: GpuGridModel>() -> Self {
        Self {
            id: Cow::Borrowed(M::ID),
            workgroup_size: M::WORKGROUP_SIZE,
            buffers: M::BUFFERS,
            step: GridShaderSpec::borrowed(M::STEP_SHADER, M::STEP_BINDINGS),
            display: GridShaderSpec::borrowed(M::DISPLAY_SHADER, M::DISPLAY_BINDINGS),
            reduce: GridShaderSpec::borrowed(M::REDUCE_SHADER, M::REDUCE_BINDINGS),
            actions: M::ACTIONS
                .iter()
                .map(|action| GridActionSpec {
                    id: Cow::Borrowed(action.desc.id()),
                    shader: GridShaderSpec::borrowed(action.shader, action.bindings),
                })
                .collect(),
            stats: Cow::Borrowed(M::STATS),
            hooks: Box::new(ModelHooks::<M>(PhantomData)),
        }
    }

    /// Returns the grid size for `params`, each side clamped to at least 1.
    pub(crate) fn dims(&self, params: &[ParamValue]) -> (u32, u32) {
        let (width, height) = self.hooks.dims(params);
        (width.max(1), height.max(1))
    }

    /// Resources that would be allocated for this model based on `params`.
    pub(crate) fn demand(&self, params: &[ParamValue], limits: &wgpu::Limits) -> Demand {
        let (width, height) = self.dims(params);

        let mut demand = Demand::default();
        for (k, len) in self.hooks.buffer_lens(width, height).into_iter().enumerate() {
            demand.push_sides(&format!("{}_buffer{k}", self.id), len, true);
        }
        demand.set_display(width, height, limits);
        for (label, storage) in self.declared_passes() {
            demand.push_pass(label, storage);
        }
        demand
    }

    /// Storage buffers each pass binds. Read by both [`Self::demand`] and [`Self::max_storage_bindings`], so the
    /// device that a host requests and the shortfall that the UI reports cannot disagree.
    fn declared_passes(&self) -> Vec<(String, u32)> {
        let mut passes = vec![
            (format!("{}_step", self.id), storage_bindings(&self.step.bindings)),
            (format!("{}_display", self.id), storage_bindings(&self.display.bindings)),
            (format!("{}_reduce", self.id), storage_bindings(&self.reduce.bindings)),
        ];
        for action in &self.actions {
            passes.push((
                format!("{}_action_{}", self.id, action.id),
                storage_bindings(&action.shader.bindings),
            ));
        }
        passes
    }

    /// Returns the most storage buffers any pass binds.
    pub(crate) fn max_storage_bindings(&self) -> u32 {
        self.declared_passes()
            .into_iter()
            .map(|(_, storage)| storage)
            .max()
            .unwrap_or(0)
    }

    /// Asserts that the buffer labels and every shader's workgroup size agree with what the engine dispatches.
    ///
    /// # Panics
    ///
    /// Panics if a buffer label is reserved or ends in `_in` or `_out`, or if a shader declares a workgroup size
    /// other than [`Self::workgroup_size`] on both sides.
    pub(crate) fn assert_contracts(&self) {
        assert_buffer_labels(&self.id, self.buffers.iter().copied());
        // Every pass dispatches square workgroups of `workgroup_size`.
        let square = [self.workgroup_size, self.workgroup_size, 1];
        let passes = [
            ("step", &self.step),
            ("display", &self.display),
            ("reduce", &self.reduce),
        ];
        for (pass, shader) in passes
            .into_iter()
            .chain(self.actions.iter().map(|action| (&*action.id, &action.shader)))
        {
            assert_workgroup_size(&self.id, pass, &shader.source, square);
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

    use super::{GpuGridSpec, GridShaderSpec};

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

    /// Returns the declarations of `bindings` as names and kinds, which compare.
    fn declared(bindings: &[BindingDecl]) -> Vec<(&'static str, BindingKind)> {
        bindings.iter().map(|decl| (decl.name, decl.kind)).collect()
    }

    /// Returns whether `shader` borrows `source` and the declarations of `bindings`.
    fn borrows(shader: &GridShaderSpec, source: &str, bindings: &[BindingDecl]) -> bool {
        matches!(shader.source, Cow::Borrowed(text) if text == source)
            && matches!(shader.bindings, Cow::Borrowed(decls) if declared(decls) == declared(bindings))
    }

    /// The spec of a model borrows every declaration it holds, and each hook calls the function of the same name.
    #[test]
    fn a_model_spec_borrows_its_declarations_and_calls_its_functions() {
        let spec = GpuGridSpec::from_model::<Probe>();
        assert!(matches!(spec.id, Cow::Borrowed("probe")), "id");
        assert_eq!(spec.workgroup_size, 8);
        assert_eq!(spec.buffers, ["state", "rng"]);
        assert!(borrows(&spec.step, Probe::STEP_SHADER, Probe::STEP_BINDINGS), "step");
        assert!(
            borrows(&spec.display, Probe::DISPLAY_SHADER, Probe::DISPLAY_BINDINGS),
            "display"
        );
        assert!(
            borrows(&spec.reduce, Probe::REDUCE_SHADER, Probe::REDUCE_BINDINGS),
            "reduce"
        );
        assert_eq!(spec.actions.len(), 1);
        let action = &spec.actions[0];
        assert!(matches!(action.id, Cow::Borrowed("spark")), "action id");
        assert!(
            borrows(&action.shader, Probe::ACTIONS[0].shader, Probe::ACTIONS[0].bindings),
            "action"
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
        assert_eq!(spec.hooks.step_params_bytes(12, 10, &params, Some(7)), vec![12, 10]);
        assert_eq!(spec.hooks.action_params_bytes(0, 12, 10, &params, 9), vec![0, 9]);
        let values: Vec<f64> = spec
            .hooks
            .stats(&[3, 5])
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
        let passes: Vec<(&str, u32)> = demand
            .passes
            .iter()
            .map(|pass| (pass.label.as_str(), pass.storage))
            .collect();
        assert_eq!(
            passes,
            [
                ("probe_step", 4),
                ("probe_display", 1),
                ("probe_reduce", 2),
                ("probe_action_spark", 1),
            ]
        );
        assert_eq!(spec.max_storage_bindings(), 4);
    }

    #[test]
    #[should_panic(expected = "probe: the spark shader declares @workgroup_size(16, 16, 1)")]
    fn an_action_shader_of_another_workgroup_size_is_refused() {
        let mut spec = GpuGridSpec::from_model::<Probe>();
        spec.actions[0].shader.source = Cow::Owned("@compute @workgroup_size(16, 16) fn main() {}".to_owned());
        spec.assert_contracts();
    }
}
