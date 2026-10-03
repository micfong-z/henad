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
use henad_core::explore::value::ValueError;
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
            ValueError::NotAnInteger { source: None, .. }
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
        ValueError::UnknownOption { .. }
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
        ValueError::NotAnInteger { .. }
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

/// Fails to compile once `ParamValue` gains a fourth `From` impl, such as `From<usize>`. An unsuffixed literal then
/// no longer infers one type.
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

/// A GPU model that steps `inner`, and asks the device for a buffer over its limit on every live parameter edit.
///
/// A live edit submits nothing and waits for nothing. Only the error scopes around the call can then report the error
/// to it. Without them the error lands in the context's sink, for whichever holder waits next.
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
        drop(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_simulation_test_oversized"),
            size: self.device.limits().max_buffer_size + 4096,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        }));
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

/// Returns `gpu_game_of_life` wrapped so that a live parameter edit raises a validation error.
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
