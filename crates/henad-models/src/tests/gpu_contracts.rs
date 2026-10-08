//! GPU models whose declarations disagree with their shaders, rejected when the engine builds them.

use henad_compute::fault::{FaultKind, catching};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::agent_engine::GpuAgentState;
use henad_compute::gpu::grid_engine::GpuGridState;
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{
    BufferSpec, DisplaySpec, Domain, Geometry, GpuAgentAction, GpuAgentModel, PassCtx, PassId, PassSpec, ReduceSpec,
};
use henad_core::authoring::model::gpu_grid_model::{GpuGridAction, GpuGridModel};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};
use henad_explore::testing::{TestDeviceRequest, headless_test_device};

use crate::gpu_ants::GpuAnts;
use crate::gpu_boids::GpuBoids;
use crate::gpu_game_of_life::GpuGameOfLife;

/// Declares a copy of [`GpuGameOfLife`] with its own workgroup size and buffer labels.
macro_rules! game_of_life_with {
    ($name:ident, $workgroup_size:expr, $buffers:expr) => {
        struct $name;

        impl GpuGridModel for $name {
            const NAME: &'static str = stringify!($name);
            const ID: &'static str = stringify!($name);
            const DESCRIPTION: &'static str = "A deliberately broken model, built only by tests";
            const PALETTE: &'static [[u8; 4]] = GpuGameOfLife::PALETTE;
            const WORKGROUP_SIZE: u32 = $workgroup_size;
            const STATS: &'static [StatDescriptor] = GpuGameOfLife::STATS;
            const ACTIONS: &'static [GpuGridAction] = GpuGameOfLife::ACTIONS;
            const BUFFERS: &'static [&'static str] = $buffers;
            const STEP_BINDINGS: &'static [BindingDecl] = GpuGameOfLife::STEP_BINDINGS;
            const DISPLAY_BINDINGS: &'static [BindingDecl] = GpuGameOfLife::DISPLAY_BINDINGS;
            const REDUCE_BINDINGS: &'static [BindingDecl] = GpuGameOfLife::REDUCE_BINDINGS;
            const STEP_SHADER: &'static str = GpuGameOfLife::STEP_SHADER;
            const DISPLAY_SHADER: &'static str = GpuGameOfLife::DISPLAY_SHADER;
            const REDUCE_SHADER: &'static str = GpuGameOfLife::REDUCE_SHADER;

            fn param_descriptors() -> Vec<ParamDescriptor> {
                GpuGameOfLife::param_descriptors()
            }

            fn dims(params: &[ParamValue]) -> (u32, u32) {
                GpuGameOfLife::dims(params)
            }

            fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
                GpuGameOfLife::buffer_lens(width, height)
            }

            fn step_dims(width: u32, height: u32) -> (u32, u32) {
                GpuGameOfLife::step_dims(width, height)
            }

            fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
                GpuGameOfLife::seed_buffers(width, height, params, seed)
            }

            fn step_params_bytes(width: u32, height: u32, params: &[ParamValue]) -> Vec<u8> {
                GpuGameOfLife::step_params_bytes(width, height, params)
            }

            fn stats(counts: &[u32]) -> Vec<StatValue> {
                GpuGameOfLife::stats(counts)
            }
        }
    };
}

game_of_life_with!(SmallWorkgroups, 8, &["state"]);
game_of_life_with!(ReservedLabel, 16, &["partials"]);
game_of_life_with!(SuffixedLabel, 16, &["state_in"]);

/// Declares a copy of the GPU agent model `$base` with its own buffers, step passes and display.
macro_rules! agent_model_with {
    (
        $name:ident,
        $base:ty,
        buffers: $buffers:expr,
        step_passes: $step_passes:expr,
        display: $display:expr $(,)?
    ) => {
        struct $name;

        impl GpuAgentModel for $name {
            const NAME: &'static str = stringify!($name);
            const ID: &'static str = stringify!($name);
            const DESCRIPTION: &'static str = "A deliberately broken model, built only by tests";
            const STATS: &'static [StatDescriptor] = <$base>::STATS;
            const BUFFERS: &'static [BufferSpec] = $buffers;
            const POS_BUFFER: usize = <$base>::POS_BUFFER;
            const COLOR_BUFFER: usize = <$base>::COLOR_BUFFER;
            const INDEX: bool = <$base>::INDEX;
            const COUNTERS: usize = <$base>::COUNTERS;
            const STEP_PASSES: &'static [PassSpec] = $step_passes;
            const DISPLAY: Option<DisplaySpec> = $display;
            const ACTIONS: &'static [GpuAgentAction] = <$base>::ACTIONS;
            const REDUCE: ReduceSpec = <$base>::REDUCE;
            const REPLAYS_EXACTLY: bool = <$base>::REPLAYS_EXACTLY;

            fn param_descriptors() -> Vec<ParamDescriptor> {
                <$base>::param_descriptors()
            }

            fn dims(params: &[ParamValue]) -> (u32, Extent) {
                <$base>::dims(params)
            }

            fn buffer_lens(geom: &Geometry) -> Vec<usize> {
                <$base>::buffer_lens(geom)
            }

            fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
                <$base>::seed_buffers(geom, params, seed)
            }

            fn index_cell_size(params: &[ParamValue]) -> f32 {
                <$base>::index_cell_size(params)
            }

            fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
                <$base>::pass_params_bytes(pass, ctx, params)
            }

            fn stats(sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue> {
                <$base>::stats(sums, counters, geom)
            }
        }
    };
}

// GpuBoids with its `color` buffer labelled `partials`.
agent_model_with!(
    ReservedAgentLabel,
    GpuBoids,
    buffers: &[
        BufferSpec {
            label: "pos",
            double_buffered: true,
            drawable: true,
        },
        BufferSpec {
            label: "vel",
            double_buffered: true,
            drawable: false,
        },
        BufferSpec {
            label: "partials",
            double_buffered: true,
            drawable: true,
        },
    ],
    step_passes: GpuBoids::STEP_PASSES,
    display: GpuBoids::DISPLAY,
);
// GpuBoids with a step shader of 16 by 16 workgroups.
agent_model_with!(
    WideStepWorkgroups,
    GpuBoids,
    buffers: GpuBoids::BUFFERS,
    step_passes: &[PassSpec {
        label: "step",
        shader: crate::shader_bindings::gpu_ants::display::SHADER_STRING,
        bindings: GpuBoids::STEP_PASSES[0].bindings,
        domain: Domain::Agents,
    }],
    display: GpuBoids::DISPLAY,
);
// GpuAnts with a display of 8 by 8 workgroups under a 16 by 16 shader.
agent_model_with!(
    SmallDisplayWorkgroups,
    GpuAnts,
    buffers: GpuAnts::BUFFERS,
    step_passes: GpuAnts::STEP_PASSES,
    display: Some(DisplaySpec {
        workgroup: 8,
        ..GpuAnts::DISPLAY.expect("ants draws a display")
    }),
);

/// Returns the panic message of `build`, which builds a model on a device.
fn build_refusal(build: impl FnOnce()) -> String {
    let fault = catching("testing", build).expect_err("the build is refused");
    match fault.kind {
        FaultKind::Panic { message, .. } => message,
        _ => panic!("expected the build to panic, got {fault:?}"),
    }
}

/// Returns the default value of each descriptor in `descriptors`.
fn defaults(descriptors: &[ParamDescriptor]) -> Vec<ParamValue> {
    descriptors
        .iter()
        .map(|descriptor| descriptor.kind.default_value())
        .collect()
}

/// Returns the panic message of building the grid model `M` on `ctx`.
fn grid_refusal<M: GpuGridModel>(ctx: &GpuContext) -> String {
    let params = defaults(&M::param_descriptors());
    build_refusal(|| drop(GpuGridState::<M>::new(ctx, &params)))
}

/// Returns the panic message of building the agent model `M` on `ctx`.
fn agent_refusal<M: GpuAgentModel>(ctx: &GpuContext) -> String {
    let params = defaults(&M::param_descriptors());
    build_refusal(|| drop(GpuAgentState::<M>::new(ctx, &params)))
}

/// Without the check, a shader at 16 by 16 under a `WORKGROUP_SIZE` of 8 validates, and every pass covers a domain
/// different from the one it is dispatched over.
#[test]
fn a_workgroup_size_the_shaders_do_not_declare_is_refused() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let message = grid_refusal::<SmallWorkgroups>(&ctx);
    assert!(
        message
            .contains("declares @workgroup_size(16, 16, 1), and the engine dispatches it in workgroups of (8, 8, 1)"),
        "{message}"
    );
}

/// Without the check, a buffer labelled `partials` is never bound, and a buffer labelled `state_in` causes a panic that
/// refers to a different label.
#[test]
fn a_reserved_or_suffixed_label_is_refused() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let reserved = grid_refusal::<ReservedLabel>(&ctx);
    assert!(reserved.contains("buffer label `partials` is reserved"), "{reserved}");
    let suffixed = grid_refusal::<SuffixedLabel>(&ctx);
    assert!(
        suffixed.contains("buffer label `state_in` ends in `_in` or `_out`"),
        "{suffixed}"
    );
}

#[test]
fn the_agent_engine_refuses_a_reserved_label() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let message = agent_refusal::<ReservedAgentLabel>(&ctx);
    assert!(message.contains("buffer label `partials` is reserved"), "{message}");
}

#[test]
fn the_agent_engine_refuses_a_step_shader_of_another_workgroup_size() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let message = agent_refusal::<WideStepWorkgroups>(&ctx);
    assert!(
        message.contains(
            "the step shader declares @workgroup_size(16, 16, 1), and the engine dispatches it in \
                          workgroups of (256, 1, 1)"
        ),
        "{message}"
    );
}

#[test]
fn the_agent_engine_refuses_a_display_shader_of_another_workgroup_size() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let message = agent_refusal::<SmallDisplayWorkgroups>(&ctx);
    assert!(
        message.contains(
            "the display shader declares @workgroup_size(16, 16, 1), and the engine dispatches it in \
                          workgroups of (8, 8, 1)"
        ),
        "{message}"
    );
}
