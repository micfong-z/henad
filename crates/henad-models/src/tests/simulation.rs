//! Checked setups and the simulations they build, over the example models.

use std::ops::ControlFlow;
use std::sync::Arc;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::FaultKind;
use henad_compute::gpu::{GpuContext, GpuSimState, StatsPoll};
use henad_compute::simulation::{RunSetup, SetupError};
use henad_compute::snapshot::GpuSnapshot;
use henad_core::action::{Schedule, Scheduled};
use henad_core::explore::replay::Replay;
use henad_core::explore::value::{ValueError, ValueKind};
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::StatEntry;

use crate::example_models;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};

fn entry(id: &str) -> ModelEntry {
    example_models().get(id).cloned().expect("the model is registered")
}

/// Returns a 64 by 64 SIR setup with seed 3.
fn small_sir() -> RunSetup {
    entry("sir")
        .setup()
        .set("grid_width", 64u32)
        .and_then(|setup| setup.set("grid_height", 64u32))
        .expect("the grid fits")
        .with_seed(3)
}

/// Checks that a tick-0 action has fired by the time the build returns, and fires once.
///
/// A press on a fresh build draws from the action stream's start, as the scheduled entry did, so the two agree.
#[test]
fn a_tick_zero_action_is_in_the_state_build_returns() {
    let mut plain = small_sir().build(None).expect("SIR builds");
    let mut acted = small_sir()
        .act_at("seed_outbreak", 0)
        .expect("SIR declares the action")
        .build(None)
        .expect("SIR builds");
    let infected = |sample: henad_compute::simulation::StatSample| sample.scalar("Infected").expect("SIR counts it");
    let before = infected(plain.stats().expect("a sample"));
    let after = infected(acted.stats().expect("a sample"));
    assert!(
        after > before,
        "the outbreak infected more cells ({before} then {after})"
    );

    acted.run_to(0).expect("nothing to step");
    assert_eq!(infected(acted.stats().expect("a sample")), after, "tick 0 fired once");
    plain.act("seed_outbreak").expect("SIR declares the action");
    assert_eq!(infected(plain.stats().expect("a sample")), after);

    acted.run_for(5).expect("SIR steps");
    plain.run_for(5).expect("SIR steps");
    assert_eq!(acted.stats().expect("a sample").entries().len(), 3);
    assert_eq!(
        infected(acted.stats().expect("a sample")),
        infected(plain.stats().expect("a sample")),
        "the two runs stay together"
    );
}

/// Checks that a panic in `run_sampled`'s callback unwinds out of the call as the caller's own panic, with no fault
/// in place of it.
#[test]
fn a_panicking_sample_callback_unwinds_as_a_panic() {
    let mut simulation = small_sir().build(None).expect("SIR builds");
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        simulation.run_sampled(10, 2, |sample| -> ControlFlow<()> {
            assert!(sample.tick() < 4, "the callback panics at tick {}", sample.tick());
            ControlFlow::Continue(())
        })
    }));
    let payload = unwound.expect_err("the callback's panic reaches the caller");
    let message = payload.downcast_ref::<String>().cloned().unwrap_or_default();
    assert_eq!(message, "the callback panics at tick 4");
}

/// Checks that a `Break` at the first sample steps nothing.
#[test]
fn a_break_at_the_first_sample_leaves_the_tick() {
    let mut simulation = small_sir().build(None).expect("SIR builds");
    simulation.run_to(3).expect("SIR steps");
    let flow = simulation
        .run_sampled(10, 1, |sample| ControlFlow::Break(sample.tick()))
        .expect("SIR samples");
    assert_eq!(flow, ControlFlow::Break(3));
    assert_eq!(simulation.tick(), 3);
}

/// Returns the reason inside a [`SetupError::Param`] that names parameter `id`.
fn param_reason(error: SetupError, id: &str) -> ValueError {
    match error {
        SetupError::Param(ValueError::Param { id: refused, source }) if refused == id => *source,
        other => panic!("expected a refusal of '{id}', got {other:?}"),
    }
}

#[test]
fn a_setup_refuses_a_value_of_the_wrong_kind() {
    let sir = entry("sir");
    let error = sir
        .setup()
        .set("grid_width", 0.5f32)
        .expect_err("a float for an integer");
    assert!(
        matches!(
            param_reason(error, "grid_width"),
            ValueError::WrongKind {
                expected: ValueKind::U32,
                found: ValueKind::F32,
                ..
            }
        ),
        "an integer parameter refuses a float"
    );
    let error = sir.setup().set("infection_rate", 2.0f32).expect_err("out of bounds");
    assert!(matches!(
        param_reason(error, "infection_rate"),
        ValueError::OutOfRange { .. }
    ));
    let error = sir.setup().set_text("infection_rate", "a").expect_err("not a number");
    assert!(matches!(
        param_reason(error, "infection_rate"),
        ValueError::NotANumber { .. }
    ));
    let error = sir.setup().set("no_such_param", 1u32).expect_err("an unknown id");
    assert!(
        matches!(error, SetupError::Param(ValueError::UnknownParam { .. })),
        "{error:?}"
    );
    let error = sir.setup().act_at("no_such_action", 3).expect_err("an unknown action");
    assert!(matches!(error, SetupError::UnknownAction { id } if id == "no_such_action"));

    let network = entry("virus_network");
    let error = network.setup().set("network", 1u32).expect_err("a u32 for a choice");
    assert!(matches!(
        param_reason(error, "network"),
        ValueError::WrongKind {
            expected: ValueKind::Choice,
            ..
        }
    ));
    let by_index = network
        .setup()
        .set("network", ParamValue::Choice(1))
        .expect("an option index");
    let by_name = network
        .setup()
        .set_text("network", "Geometric")
        .expect("an option name");
    assert_eq!(by_index.values(), by_name.values());

    let mut simulation = small_sir().build(None).expect("SIR builds");
    let error = simulation
        .set_param("grid_width", 32u32)
        .expect_err("the grid size applies on a build");
    assert!(
        matches!(&error, SetupError::ReloadOnly { id } if id == "grid_width"),
        "{error:?}"
    );
    let error = simulation
        .set_param("infection_rate", -1.0f32)
        .expect_err("out of bounds");
    assert!(matches!(
        param_reason(error, "infection_rate"),
        ValueError::OutOfRange { .. }
    ));
    simulation
        .set_param("infection_rate", 0.1f32)
        .expect("a live parameter");
    assert_eq!(
        simulation.setup().values(),
        small_sir().values(),
        "a live edit leaves the setup alone"
    );
}

#[test]
fn a_setup_refuses_a_wrong_value_count() {
    let sir = entry("sir");
    let values = sir.setup().values().to_vec();
    let count = values.len();
    let error = RunSetup::from_parts(&sir, &values[1..], None, Schedule::default()).expect_err("a value short");
    assert!(
        matches!(error, SetupError::ParamCount { expected, found } if expected == count && found == count - 1),
        "{error:?}"
    );

    let mut wrong_kind = values.clone();
    wrong_kind[0] = ParamValue::F32(64.0);
    let error = RunSetup::from_parts(&sir, &wrong_kind, None, Schedule::default()).expect_err("a float width");
    assert!(matches!(
        param_reason(error, "grid_width"),
        ValueError::WrongKind {
            expected: ValueKind::U32,
            ..
        }
    ));

    let misnamed = Schedule::from_entries(vec![Scheduled {
        index: 0,
        id: "clear".to_owned(),
        tick: 2,
    }]);
    let error = RunSetup::from_parts(&sir, &values, None, misnamed).expect_err("an id of another model");
    assert!(
        matches!(&error, SetupError::UnknownAction { id } if id == "clear"),
        "{error:?}"
    );

    let replay = Replay {
        model: "game_of_life".to_owned(),
        params: values.clone(),
        seed: 1,
        schedule: Schedule::default(),
        ticks: 10,
        label: String::new(),
    };
    let error = RunSetup::from_replay(&sir, &replay).expect_err("a run of another model");
    assert!(matches!(error, SetupError::WrongModel { .. }), "{error:?}");
    let setup = RunSetup::from_replay(
        &sir,
        &Replay {
            model: "sir".to_owned(),
            ..replay
        },
    )
    .expect("a run of SIR");
    assert_eq!((setup.values(), setup.seed()), (values.as_slice(), Some(1)));
}

/// Fails to compile once `ParamValue` gains a fourth `From` impl for an integer type other than `i32`, such as
/// `From<usize>`. An unsuffixed literal then no longer infers one type.
///
/// An impl for `i32` or `f64`, the types an unsuffixed literal falls back to, would still compile here. A static check
/// beside the test refuses those two.
#[test]
fn an_unsuffixed_literal_sets_a_parameter() {
    let sir = entry("sir");
    let setup = sir
        .setup()
        .set("grid_width", 256)
        .and_then(|setup| setup.set("infection_rate", 0.4))
        .expect("both fit");
    let infection_rate = sir.param_index("infection_rate").expect("SIR has an infection rate");
    assert_eq!(setup.values()[0], ParamValue::U32(256));
    assert_eq!(setup.values()[infection_rate], ParamValue::F32(0.4));
}

// Fails to compile once `ParamValue` implements `From<f64>` or `From<i32>`. The path below then matches two impls of
// `AmbiguousIfFrom`, and its marker cannot be inferred.
const _: fn() = || {
    trait AmbiguousIfFrom<Marker> {
        fn some_item() {}
    }
    impl<T> AmbiguousIfFrom<()> for T {}
    struct FromF64;
    impl<T: From<f64>> AmbiguousIfFrom<FromF64> for T {}
    struct FromI32;
    impl<T: From<i32>> AmbiguousIfFrom<FromI32> for T {}
    let _ = <ParamValue as AmbiguousIfFrom<_>>::some_item;
};

/// Checks that the ants field read through `views` is the field of the tick it is read at, not the one last
/// quantised.
#[test]
fn views_are_prepared_at_the_read() {
    let ants = entry("ants");
    let setup = ants
        .setup()
        .set("num_agents", 2000u32)
        .and_then(|setup| setup.set("world_width", 64.0f32))
        .and_then(|setup| setup.set("world_height", 64.0f32))
        .expect("the sizes fit")
        .with_seed(9);
    let field = |simulation: &mut henad_compute::simulation::Simulation| {
        let views = simulation.views().expect("the views prepare");
        views.grid.expect("ants has a field").cells.to_vec()
    };

    let mut read_late = setup.build(None).expect("ants builds");
    read_late.run_to(40).expect("ants steps");
    let at_forty = field(&mut read_late);

    let mut read_twice = setup.build(None).expect("ants builds");
    let at_zero = field(&mut read_twice);
    read_twice.run_to(40).expect("ants steps");
    assert_eq!(
        field(&mut read_twice),
        at_forty,
        "the second read prepares tick 40's field"
    );
    assert_ne!(at_zero, at_forty, "the field moved between the two reads");

    let mut exported = Vec::new();
    read_twice
        .write_state(&mut exported)
        .expect("a CPU model writes its state");
    assert!(!exported.is_empty());
}

/// A GPU model that steps `inner`, and asks the device for a buffer over its limit on every live parameter edit and,
/// from another thread, on every action.
///
/// A live edit submits nothing and waits for nothing. Only the error scopes around the call can then report the error
/// to it. Without them the error lands in the context's sink, for whichever holder waits next. Error scopes are
/// thread-local, and an action's error lands in the sink whatever scopes its call pushes.
struct FaultyGpu {
    inner: Box<dyn GpuSimState>,
    device: wgpu::Device,
}

impl SimState for FaultyGpu {
    fn step(&mut self) {
        self.inner.step();
    }
    fn tick(&self) -> u64 {
        self.inner.tick()
    }
    fn stats(&self) -> Vec<StatEntry> {
        self.inner.stats()
    }
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        request_oversized_buffer(&self.device);
        self.inner.set_param(index, value)
    }
    fn population(&self) -> u64 {
        self.inner.population()
    }
    fn heap_bytes(&self) -> usize {
        self.inner.heap_bytes()
    }
}

impl GpuSimState for FaultyGpu {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        self.inner.encode_steps(encoder, count, timestamps);
    }
    fn encode_action(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> bool {
        std::thread::scope(|scope| scope.spawn(|| request_oversized_buffer(&self.device)).join())
            .expect("the request returns");
        self.inner.encode_action(encoder, index)
    }
    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.inner.encode_snapshot_passes(encoder);
    }
    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.inner.encode_stats_passes(encoder);
    }
    fn begin_stats_readback(&mut self) {
        self.inner.begin_stats_readback();
    }
    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        self.inner.poll_stats_readback(device, block)
    }
    fn stats_readback_pending(&self) -> bool {
        self.inner.stats_readback_pending()
    }
    fn view(&self) -> GpuSnapshot {
        self.inner.view()
    }
}

/// Asks `device` for a buffer over its limit, which raises a validation error.
fn request_oversized_buffer(device: &wgpu::Device) {
    drop(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("henad_simulation_test_oversized"),
        size: device.limits().max_buffer_size + 4096,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    }));
}

/// Returns `gpu_game_of_life` wrapped so that a live parameter edit and an action raise a validation error.
fn faulty_life() -> ModelEntry {
    entry("gpu_game_of_life").wrap_factory(|factory| {
        Arc::new(
            move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| {
                let ModelState::Gpu(inner) = factory(params, seed, gpu)? else {
                    panic!("gpu_game_of_life builds a GPU state");
                };
                let device = gpu.expect("a GPU model builds on a device").device.clone();
                Ok(ModelState::Gpu(Box::new(FaultyGpu { inner, device })))
            },
        )
    })
}

/// Checks that a device error comes back from the call that raised it, on a plain test thread, and leaves a second
/// simulation on the same device and the device's sink alone.
///
/// The error is raised by a live parameter edit, which waits for nothing. Without the error scopes around the call
/// the edit would return without the error, and the healthy simulation's next wait would report it from the sink.
#[test]
fn a_gpu_simulation_reports_its_own_device_error() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let small = |entry: ModelEntry| {
        entry
            .setup()
            .set("grid_width", 64u32)
            .and_then(|setup| setup.set("grid_height", 64u32))
            .expect("the grid fits")
            .with_seed(1)
    };
    let mut healthy = small(entry("gpu_game_of_life")).build(Some(&ctx)).expect("builds");
    let mut faulty = small(faulty_life()).build(Some(&ctx)).expect("builds");
    healthy.run_for(5).expect("the healthy simulation steps");
    faulty.run_for(5).expect("the faulty simulation steps");

    let error = faulty
        .set_param("grid_width", 64u32)
        .expect_err("the oversized buffer is reported");
    let SetupError::Fault(fault) = error else {
        panic!("expected the device error as a fault, got {error:?}");
    };
    assert!(matches!(fault.kind, FaultKind::Device(_)), "{fault:?}");
    healthy.run_for(20).expect("the healthy simulation steps on");
    assert_eq!(healthy.tick(), 25);
    assert!(ctx.faults.take().is_none(), "no error reached the device's sink");
}

/// Returns a 64 by 64 `gpu_game_of_life` setup with seed 1.
fn small_gpu_life() -> RunSetup {
    entry("gpu_game_of_life")
        .setup()
        .set("grid_width", 64u32)
        .and_then(|setup| setup.set("grid_height", 64u32))
        .expect("the grid fits")
        .with_seed(1)
}

/// Checks that a lost device fails the stepping and the sampling of every simulation on it.
///
/// wgpu reports a loss to no error scope, and polls go on succeeding. The regression returned `Ok` from both, advanced
/// the tick and repeated the last stats it had read.
#[test]
fn a_lost_device_fails_the_next_step_and_sample() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let mut stepped = small_gpu_life().build(Some(&ctx)).expect("builds");
    let mut sampled = small_gpu_life().build(Some(&ctx)).expect("builds");
    stepped.run_for(5).expect("the simulation steps");
    sampled.run_for(5).expect("the simulation steps");
    ctx.device.destroy();

    let fault = stepped.run_for(5).expect_err("the loss fails the step");
    assert!(matches!(fault.kind, FaultKind::DeviceLost), "{fault:?}");
    let fault = stepped.stats().expect_err("the loss fails every later call");
    assert!(matches!(fault.kind, FaultKind::DeviceLost), "{fault:?}");
    let fault = sampled.stats().expect_err("the loss fails the sample");
    assert!(matches!(fault.kind, FaultKind::DeviceLost), "{fault:?}");
    let fault = sampled
        .run_sampled(20, 5, |_| ControlFlow::<()>::Continue(()))
        .expect_err("the loss fails a sampled run");
    assert!(matches!(fault.kind, FaultKind::DeviceLost), "{fault:?}");
}

/// SIR's CPU state, panicking at the start of the step from tick 2, in place of a kernel that panics part way.
struct PanicsOnce {
    inner: Box<dyn SimState>,
    panicked: bool,
}

impl SimState for PanicsOnce {
    fn step(&mut self) {
        if self.inner.tick() == 2 && !self.panicked {
            self.panicked = true;
            panic!("the model breaks at tick 2");
        }
        self.inner.step();
    }
    fn tick(&self) -> u64 {
        self.inner.tick()
    }
    fn stats(&self) -> Vec<StatEntry> {
        self.inner.stats()
    }
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.inner.set_param(index, value)
    }
    fn act(&mut self, index: usize) -> bool {
        self.inner.act(index)
    }
    fn population(&self) -> u64 {
        self.inner.population()
    }
    fn heap_bytes(&self) -> usize {
        self.inner.heap_bytes()
    }
}

/// Checks that a simulation refuses every call that runs model code once a call has faulted.
///
/// The regression stepped on from the half-done step and returned `Ok`, reaching a state no rebuild reproduces.
#[test]
fn a_simulation_refuses_every_call_after_a_fault() {
    let broken = entry("sir").wrap_factory(|factory| {
        Arc::new(
            move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| {
                let ModelState::Cpu(inner) = factory(params, seed, gpu)? else {
                    panic!("sir builds a CPU state");
                };
                Ok(ModelState::Cpu(Box::new(PanicsOnce { inner, panicked: false })))
            },
        )
    });
    let mut simulation = broken
        .setup()
        .set("grid_width", 64u32)
        .and_then(|setup| setup.set("grid_height", 64u32))
        .expect("the grid fits")
        .build(None)
        .expect("SIR builds");
    let fault = simulation.run_for(10).expect_err("the step panics");
    assert!(matches!(fault.kind, FaultKind::Panic { .. }), "{fault:?}");
    assert_eq!(simulation.tick(), 2);

    let refused = |fault: henad_compute::fault::Fault| {
        assert!(
            matches!(&fault.kind, FaultKind::Refused(message) if message.contains("tick 2")),
            "{fault:?}"
        );
    };
    refused(simulation.run_for(1).expect_err("a step after the fault"));
    refused(simulation.run_to(5).expect_err("a run after the fault"));
    refused(simulation.stats().expect_err("a sample after the fault"));
    refused(
        simulation
            .run_sampled(5, 1, |_| ControlFlow::<()>::Continue(()))
            .expect_err("a sampled run after the fault"),
    );
    refused(simulation.views().expect_err("the views after the fault"));
    refused(simulation.relax_layout(1.0).expect_err("a layout after the fault"));
    for error in [
        simulation
            .set_param("infection_rate", 0.5f32)
            .expect_err("a live edit after the fault"),
        simulation.act("seed_outbreak").expect_err("an action after the fault"),
    ] {
        let SetupError::Fault(fault) = error else {
            panic!("expected the earlier fault, got {error:?}");
        };
        refused(fault);
    }
    assert_eq!(simulation.tick(), 2, "nothing stepped after the fault");
}

/// Returns the scalar of every stat `simulation` reports at its current tick.
fn scalars(simulation: &mut henad_compute::simulation::Simulation) -> Vec<f64> {
    let sample = simulation.stats().expect("a sample");
    sample.entries().iter().map(|entry| entry.value.scalar()).collect()
}

/// Checks that a live edit reaches the parameter it names, as the same value set before the build does.
#[test]
fn a_live_edit_steps_as_the_same_value_set_at_the_build() {
    let mut live = small_sir().build(None).expect("SIR builds");
    live.set_param("infection_rate", 0.6f32).expect("a live parameter");
    live.run_for(30).expect("SIR steps");

    let built = small_sir().set("infection_rate", 0.6f32).expect("in bounds");
    let mut built = built.build(None).expect("SIR builds");
    built.run_for(30).expect("SIR steps");

    let mut unedited = small_sir().build(None).expect("SIR builds");
    unedited.run_for(30).expect("SIR steps");

    let live = scalars(&mut live);
    assert_eq!(live, scalars(&mut built), "the live edit set another parameter");
    assert_ne!(live, scalars(&mut unedited), "the live edit changed nothing");
}

/// Checks that relaxing a network's layout moves its nodes, and that a model without a layout accepts the call.
#[test]
fn relax_layout_moves_a_network_and_leaves_other_models_alone() {
    let mut network = entry("virus_network")
        .setup()
        .set("num_agents", 300u32)
        .expect("in bounds")
        .with_seed(5)
        .build(None)
        .expect("Virus on a Network builds");
    let positions = |simulation: &mut henad_compute::simulation::Simulation| {
        let views = simulation.views().expect("the views prepare");
        let points = views.points.expect("a network has points");
        (points.pos_x.to_vec(), points.pos_y.to_vec())
    };
    let before = positions(&mut network);
    network.relax_layout(20.0).expect("the layout relaxes");
    assert_ne!(positions(&mut network), before, "the layout moved no node");

    let mut sir = small_sir().build(None).expect("SIR builds");
    sir.relax_layout(20.0)
        .expect("a model without a layout accepts the call");
}

/// Checks that an action on a GPU simulation reaches its state.
#[test]
fn an_action_on_a_gpu_simulation_reaches_its_state() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let mut simulation = small_gpu_life().build(Some(&ctx)).expect("builds");
    simulation.run_for(3).expect("the simulation steps");
    let alive = |simulation: &mut henad_compute::simulation::Simulation| {
        simulation
            .stats()
            .expect("a sample")
            .scalar("Alive")
            .expect("Game of Life counts the living")
    };
    assert!(alive(&mut simulation) > 0.0, "a seeded grid has living cells");
    simulation.act("clear").expect("gpu_game_of_life declares clear");
    assert_eq!(alive(&mut simulation), 0.0, "the clear pass ran");
    assert!(ctx.faults.take().is_none());
}

/// Checks that an action on a GPU simulation waits for the device before it returns, and reports an error that
/// reached the device's sink in the meantime.
///
/// Without the wait the action returns once it is submitted. The error stays in the sink, and the healthy
/// simulation's next wait reports it.
#[test]
fn a_gpu_action_reports_an_error_from_the_sink_before_it_returns() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        return;
    };
    let mut healthy = small_gpu_life().build(Some(&ctx)).expect("builds");
    let mut faulty = faulty_life()
        .setup()
        .set("grid_width", 64u32)
        .and_then(|setup| setup.set("grid_height", 64u32))
        .expect("the grid fits")
        .with_seed(1)
        .build(Some(&ctx))
        .expect("builds");
    healthy.run_for(5).expect("the healthy simulation steps");
    faulty.run_for(5).expect("the faulty simulation steps");

    let error = faulty.act("clear").expect_err("the action's error is reported");
    let SetupError::Fault(fault) = error else {
        panic!("expected the device error as a fault, got {error:?}");
    };
    assert!(matches!(fault.kind, FaultKind::Device(_)), "{fault:?}");
    healthy.run_for(20).expect("the healthy simulation steps on");
    assert!(ctx.faults.take().is_none(), "no error was left in the device's sink");
}
