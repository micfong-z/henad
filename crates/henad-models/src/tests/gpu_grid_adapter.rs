//! The example GPU grid models as [`GpuGridState`] reads them: the buffers and passes of a demand, and the stats, size
//! and actions of a built state, each compared with the model's own declarations.

use henad_compute::gpu::GpuContext;
use henad_compute::gpu::grid_engine::GpuGridState;
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};

use crate::gpu_game_of_life::GpuGameOfLife;
use crate::gpu_sir::GpuSir;

// Grid size of every case. A width of 48 leaves part of a word unused in a bit-packed row.
const WIDTH: u32 = 48;
const HEIGHT: u32 = 40;

/// Returns the default values of `M`'s parameters, with the grid at [`WIDTH`] by [`HEIGHT`].
fn params<M: GpuGridModel>() -> Vec<ParamValue> {
    M::param_descriptors()
        .iter()
        .map(|descriptor| match descriptor.id() {
            "grid_width" => ParamValue::U32(WIDTH),
            "grid_height" => ParamValue::U32(HEIGHT),
            _ => descriptor.kind().default_value(),
        })
        .collect()
}

fn storage_buffers(bindings: &[BindingDecl]) -> u32 {
    bindings.iter().filter(|decl| decl.kind.is_storage_buffer()).count() as u32
}

/// Checks that the demand of `M` names both sides of each declared buffer at the length `M` returns, and each pass
/// with the storage buffers its declarations bind, actions included.
fn assert_demand_follows<M: GpuGridModel>() {
    let demand = GpuGridState::<M>::demand(&params::<M>(), &wgpu::Limits::default());
    let lens = M::buffer_lens(WIDTH, HEIGHT);
    assert_eq!(lens.len(), M::BUFFERS.len(), "{}: one length per buffer", M::ID);

    let buffers: Vec<(&str, u64)> = demand
        .buffers
        .iter()
        .map(|alloc| (alloc.label.as_str(), alloc.bytes))
        .collect();
    let declared_buffers: Vec<(String, u64)> = lens
        .iter()
        .enumerate()
        .flat_map(|(k, &len)| ["a", "b"].map(|side| (format!("{}_buffer{k}_{side}", M::ID), len as u64 * 4)))
        .collect();
    let declared_buffers: Vec<(&str, u64)> = declared_buffers
        .iter()
        .map(|(label, bytes)| (label.as_str(), *bytes))
        .collect();
    assert_eq!(buffers, declared_buffers, "{}: buffers", M::ID);
    assert_eq!(demand.texture, Some((WIDTH, HEIGHT)), "{}: display texture", M::ID);

    let passes: Vec<(&str, u32)> = demand
        .passes
        .iter()
        .map(|pass| (pass.label.as_str(), pass.storage))
        .collect();
    let mut declared_passes = vec![
        (format!("{}_step", M::ID), storage_buffers(M::STEP_BINDINGS)),
        (format!("{}_display", M::ID), storage_buffers(M::DISPLAY_BINDINGS)),
        (format!("{}_reduce", M::ID), storage_buffers(M::REDUCE_BINDINGS)),
    ];
    declared_passes.extend(M::ACTIONS.iter().map(|action| {
        (
            format!("{}_action_{}", M::ID, action.desc.id()),
            storage_buffers(action.bindings),
        )
    }));
    let declared_passes: Vec<(&str, u32)> = declared_passes
        .iter()
        .map(|(label, storage)| (label.as_str(), *storage))
        .collect();
    assert_eq!(passes, declared_passes, "{}: passes", M::ID);

    let widest = declared_passes.iter().map(|&(_, storage)| storage).max();
    assert_eq!(
        Some(GpuGridState::<M>::max_storage_bindings()),
        widest,
        "{}: widest pass",
        M::ID
    );
}

/// The demand of each example GPU grid model holds its buffers, its display texture and every pass it declares.
#[test]
fn the_demand_of_each_gpu_grid_model_follows_its_declarations() {
    assert_demand_follows::<GpuGameOfLife>();
    assert_demand_follows::<GpuSir>();
}

/// Checks that a state of `M` built on `ctx` reports `M`'s stats, its cell count, the bytes of its buffers and its
/// display texture, and runs each declared action and no other.
fn assert_state_follows<M: GpuGridModel>(ctx: &GpuContext) {
    let mut state = GpuGridState::<M>::new(ctx, &params::<M>());

    let entries = state.stats();
    let reported: Vec<(&str, [u8; 4])> = entries.iter().map(|entry| (entry.label(), entry.color())).collect();
    let declared: Vec<(&str, [u8; 4])> = M::STATS.iter().map(|stat| (stat.label(), stat.color())).collect();
    assert_eq!(reported, declared, "{}: stat series", M::ID);
    // Every counter reads zero until the first readback.
    let values: Vec<String> = entries.iter().map(|entry| format!("{:?}", entry.value())).collect();
    let unread: Vec<String> = M::stats(&vec![0; M::STATS.len()])
        .iter()
        .map(|value| format!("{value:?}"))
        .collect();
    assert_eq!(values, unread, "{}: stat values before a readback", M::ID);

    assert_eq!(state.population(), u64::from(WIDTH) * u64::from(HEIGHT), "{}", M::ID);
    let words: usize = M::buffer_lens(WIDTH, HEIGHT).iter().sum();
    let texture = (WIDTH * HEIGHT * 4) as usize;
    assert_eq!(state.heap_bytes(), words * 4 * 2 + texture, "{}: heap bytes", M::ID);
    let printed = format!("{state:?}");
    assert!(printed.contains(&format!("model: {:?}", M::ID)), "{printed}");

    for index in 0..M::ACTIONS.len() {
        assert!(state.act(index), "{}: action {index}", M::ID);
    }
    assert!(!state.act(M::ACTIONS.len()), "{}: an undeclared action", M::ID);
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device finishes the actions");
}

/// A state of each example GPU grid model reports what its model declares.
#[test]
fn a_built_gpu_grid_state_follows_its_model_declarations() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping a_built_gpu_grid_state_follows_its_model_declarations: no adapter");
        return;
    };
    assert_state_follows::<GpuGameOfLife>(&ctx);
    assert_state_follows::<GpuSir>(&ctx);
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}
