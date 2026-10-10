//! Tests of a build on its own thread, of the commands sent while it runs, and of runs and builds that never return.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use henad_compute::cpu::sim_thread::SimCommand;
use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::{BUILDING, Fault, FaultKind};
use henad_compute::gpu::GpuContext;
use henad_compute::runner::DEFAULT_STOP_TIMEOUT;
use henad_core::explore::spec::SweepSpec;
use henad_core::params::ParamValue;

use crate::state::{AppState, BUILDING_MODEL, DID_NOT_STOP, OpenAt, STILL_STOPPING, STOP_WAIT};
use crate::tests::support::{Gate, OpenOnDrop, Probe, StepHold, accessible_texts, held, set_of};
use crate::ui::playback::playback_ui;
use crate::ui::results::ResultsPanel;
use crate::ui::sweep::session::{SessionExecution, SessionState, SweepSession};
use crate::ui::viewport::viewport_ui;

/// Time a call that must not wait on a stuck thread may take past [`STOP_WAIT`], for the scheduler.
const SLACK: Duration = Duration::from_secs(1);

/// Longest time a test waits for a thread to reach the state it expects.
const DEADLINE: Duration = Duration::from_secs(30);

/// Polls `condition` until it holds, or panics with `what` after [`DEADLINE`].
fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !condition() {
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Returns the example SIR entry.
fn sir() -> ModelEntry {
    henad_models::example_models()
        .get("sir")
        .cloned()
        .expect("the example models include sir")
}

/// Returns an app over `entry` alone, selected at a grid of 64 by 64, or `None` to skip on a machine without a device.
fn app_over(entry: ModelEntry) -> Option<AppState> {
    let mut app = AppState::headless(set_of(entry), false)?;
    for id in ["grid_width", "grid_height"] {
        let index = app
            .selected_entry()
            .and_then(|entry| entry.param_index(id))
            .expect("a grid model declares its size");
        app.param_values[index] = ParamValue::U32(64);
    }
    Some(app)
}

/// Builds the selection, waits for its runner, and steps it once. A held state's first step blocks there.
fn build_and_step(app: &mut AppState, probe: &Probe) {
    app.reset_simulation();
    app.wait_for_build();
    app.sim_thread.as_mut().expect("the run").step_once();
    wait_until("the first step", || !Probe::of(&probe.step_threads, 1).is_empty());
}

/// Drops the detached threads that have ended, and returns whether none is left.
fn stopped(app: &mut AppState) -> bool {
    app.poll_stopping(&egui::Context::default());
    app.stopping_status().is_none()
}

/// Returns whether `name` names a worker of the pool that replaces the global pool after a detach.
fn in_fresh_pool(name: &Option<String>) -> bool {
    name.as_deref().is_some_and(|name| name.starts_with("henad-pool-"))
}

/// Returns one run of SIR on a 16 by 16 grid, of 4 steps.
fn sir_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = vec![
        ("grid_width".to_owned(), "16".to_owned()),
        ("grid_height".to_owned(), "16".to_owned()),
    ];
    spec.run.steps = 4;
    spec
}

/// Returns the generation of the app's pool, and whether it is rayon's global pool.
fn pool_of(app: &AppState) -> (u64, bool) {
    let (generation, pool) = app.model_pool().current();
    (generation, pool.is_none())
}

#[test]
fn a_cpu_build_after_a_detach_runs_on_a_fresh_pool() {
    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, None, Some(StepHold::Step(gate.clone())))) else {
        return;
    };
    build_and_step(&mut app, &probe);
    assert_eq!(pool_of(&app), (0, true), "the first run uses the global pool");
    assert_eq!(
        Probe::of(&probe.build_threads, 1),
        [Some("henad-model-build".to_owned())],
        "a build runs on its own thread"
    );

    app.offload_simulation();
    assert_eq!(app.stopping_status(), Some(STILL_STOPPING));
    assert_eq!(
        pool_of(&app),
        (0, true),
        "the pool stays until the run is past the timeout"
    );

    // The build waits for the held run until the timeout, then retires the global pool.
    app.reset_simulation();
    app.wait_for_build();
    assert_eq!(pool_of(&app), (1, false), "a fresh pool after the timeout");
    let pool = app.model_pool().current().1.expect("a fresh pool");
    assert_eq!(
        pool.current_num_threads(),
        rayon::current_num_threads(),
        "the global width"
    );
    assert_eq!(app.stopping_status(), Some(DID_NOT_STOP));

    app.sim_thread.as_mut().expect("the second run").step_once();
    wait_until("the second step", || !Probe::of(&probe.step_threads, 2).is_empty());
    let build_threads = Probe::of(&probe.build_threads, 2);
    assert!(build_threads.iter().all(in_fresh_pool), "built on {build_threads:?}");
    let step_threads = Probe::of(&probe.step_threads, 2);
    assert!(step_threads.iter().all(in_fresh_pool), "stepped on {step_threads:?}");
    assert_eq!(
        Probe::of(&probe.step_threads, 1).len(),
        1,
        "the first run is still held in its step"
    );

    gate.open();
    wait_until("the end of the detached run", || stopped(&mut app));
}

#[test]
fn a_slow_runner_is_waited_for_and_keeps_the_pool() {
    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, None, Some(StepHold::Step(gate.clone())))) else {
        return;
    };
    build_and_step(&mut app, &probe);

    let start = Instant::now();
    app.reset_simulation();
    assert!(
        start.elapsed() < STOP_WAIT + SLACK,
        "Build waited {:?}",
        start.elapsed()
    );
    assert_eq!(app.stopping_status(), Some(STILL_STOPPING), "the run is detached");
    assert!(app.is_building());

    // The run ends well before the timeout, as a long batch would.
    gate.open();
    app.wait_for_build();
    assert_eq!(
        Probe::of(&probe.drops_seen, 2),
        [1],
        "the second build started before the first run had dropped its state"
    );
    assert_eq!(
        pool_of(&app),
        (0, true),
        "a run that stops before the timeout keeps the pool"
    );
    assert!(stopped(&mut app));
}

#[test]
fn a_viewport_frame_after_a_detach_runs_on_the_fresh_pool() {
    /// Time after which the watchdog releases the held global pool, and the test fails.
    const WATCHDOG: Duration = Duration::from_secs(20);

    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, None, Some(StepHold::Pool(gate.clone())))) else {
        return;
    };
    build_and_step(&mut app, &probe);
    // Every worker of the global pool takes an item of the held step. A worker busy with another test joins later.
    let workers = rayon::current_num_threads();
    let deadline = Instant::now() + Duration::from_secs(5);
    while probe.held_workers.load(Ordering::SeqCst) < workers && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }

    // A frame stuck on the held global pool waits until the watchdog opens the gate.
    let fired = Arc::new(AtomicBool::new(false));
    let (stop_watchdog, watchdog) = std::sync::mpsc::channel::<()>();
    let watchdog_gate = gate.clone();
    let watchdog_fired = Arc::clone(&fired);
    let watchdog = std::thread::spawn(move || {
        if watchdog.recv_timeout(WATCHDOG).is_err() {
            watchdog_fired.store(true, Ordering::SeqCst);
            watchdog_gate.open();
        }
    });

    app.reset_simulation();
    app.wait_for_build();
    assert_eq!(pool_of(&app), (1, false), "a fresh pool after the timeout");
    let thread = app.sim_thread.as_mut().expect("the second run");
    let mut first = None;
    wait_until("the first snapshot of the second run", || {
        first = thread.take_snapshot();
        first.is_some()
    });
    app.snapshot = first;
    egui::Context::default()
        .run_ui(egui::RawInput::default(), |ui| viewport_ui(ui, &mut app))
        .drop_without_applying_deltas();
    assert!(
        !fired.load(Ordering::SeqCst),
        "the viewport frame waited for the held global pool"
    );
    assert!(app.grid_texture.is_some(), "the frame uploaded the grid");

    drop(stop_watchdog);
    watchdog.join().expect("the watchdog never panics");
    gate.open();
    wait_until("the end of the detached run", || stopped(&mut app));
}

#[test]
fn a_viewport_frame_beside_a_run_that_holds_its_pool_returns() {
    /// Time after which the watchdog releases the held pool, and the test fails.
    const WATCHDOG: Duration = Duration::from_secs(20);

    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, None, Some(StepHold::Pool(gate.clone())))) else {
        return;
    };
    app.reset_simulation();
    app.wait_for_build();
    let thread = app.sim_thread.as_mut().expect("the run");
    let mut first = None;
    wait_until("the first snapshot", || {
        first = thread.take_snapshot();
        first.is_some()
    });
    // A step right after a publish holds every worker of the run's pool, and nothing stops the run.
    thread.step_once();
    let workers = rayon::current_num_threads();
    let deadline = Instant::now() + Duration::from_secs(5);
    while probe.held_workers.load(Ordering::SeqCst) < workers && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(pool_of(&app), (0, true), "the run holds the global pool");

    let fired = Arc::new(AtomicBool::new(false));
    let (stop_watchdog, watchdog) = std::sync::mpsc::channel::<()>();
    let watchdog_gate = gate.clone();
    let watchdog_fired = Arc::clone(&fired);
    let watchdog = std::thread::spawn(move || {
        if watchdog.recv_timeout(WATCHDOG).is_err() {
            watchdog_fired.store(true, Ordering::SeqCst);
            watchdog_gate.open();
        }
    });
    app.snapshot = first;
    egui::Context::default()
        .run_ui(egui::RawInput::default(), |ui| viewport_ui(ui, &mut app))
        .drop_without_applying_deltas();
    assert!(
        !fired.load(Ordering::SeqCst),
        "the viewport frame waited for the pool the run holds"
    );
    assert!(app.grid_texture.is_some(), "the frame uploaded the grid");

    drop(stop_watchdog);
    watchdog.join().expect("the watchdog never panics");
    gate.open();
    app.offload_simulation();
    wait_until("the end of the run", || stopped(&mut app));
}

#[test]
fn a_build_that_never_returns_leaves_the_ui_responsive() {
    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, Some(gate.clone()), None)) else {
        return;
    };

    let start = Instant::now();
    app.reset_simulation();
    assert!(start.elapsed() < SLACK, "Build waited {:?}", start.elapsed());
    wait_until("the start of the build", || probe.builds.load(Ordering::SeqCst) == 1);
    assert!(app.is_building());
    assert_eq!(
        app.loaded_id(),
        Some("sir"),
        "the panels describe the model being built"
    );

    let start = Instant::now();
    app.poll_build();
    let texts = accessible_texts(
        |ui| playback_ui(ui, &mut app),
        |node| node.value().or_else(|| node.label()),
    );
    assert!(start.elapsed() < SLACK, "a frame waited {:?}", start.elapsed());
    assert!(texts.iter().any(|text| text == BUILDING_MODEL), "{texts:?}");
    assert!(texts.iter().any(|text| text == "Cancel"), "{texts:?}");
    assert!(app.is_building(), "the build still holds its thread");

    // Cancel in the Playback panel.
    let start = Instant::now();
    app.offload_simulation();
    assert!(start.elapsed() < SLACK, "Cancel waited {:?}", start.elapsed());
    assert!(!app.is_building());
    assert_eq!(app.loaded_id(), None);
    assert_eq!(app.stopping_status(), Some(STILL_STOPPING));
    assert_eq!(app.stopping_status_after_the_timeout(), Some(DID_NOT_STOP));

    app.reset_simulation();
    app.wait_for_build();
    assert!(app.sim_thread.is_some(), "a build while the first one holds its thread");
    assert_eq!(*probe.built.lock().expect("the probe"), [2]);
    assert_eq!(
        pool_of(&app),
        (1, false),
        "a fresh pool once the abandoned build is past the timeout"
    );

    gate.open();
    wait_until("the end of the abandoned build", || stopped(&mut app));
    assert_eq!(*probe.built.lock().expect("the probe"), [2, 1]);
    wait_until("the drop of the abandoned build's state", || {
        probe.dropped.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn commands_sent_during_a_build_reach_the_new_runner() {
    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, Some(gate.clone()), None)) else {
        return;
    };
    let entry = app.selected_entry().cloned().expect("sir is selected");
    let outbreak = entry.action_index("seed_outbreak").expect("SIR declares it");
    let infection = entry.param_index("infection_rate").expect("SIR declares it");
    let setup = entry
        .setup()
        .set("grid_width", 64_u32)
        .and_then(|setup| setup.set("grid_height", 64_u32))
        .and_then(|setup| setup.act_at("seed_outbreak", 5))
        .expect("a valid setup");

    app.open_setup(&setup, OpenAt::Tick(20)).expect("the setup opens");
    assert!(app.is_building());
    assert_eq!(app.run_to_target, Some(20));
    assert!(app.send_live_param(infection, ParamValue::F32(0.3)), "a live edit");
    assert_eq!(app.loaded_values[infection], ParamValue::F32(0.3));
    app.send_command(SimCommand::Act(outbreak));

    gate.open();
    app.wait_for_build();
    let thread = app.sim_thread.as_mut().expect("the run");
    wait_until("the run to tick 20", || {
        std::iter::from_fn(|| thread.take_snapshot()).any(|snapshot| snapshot.tick == 20)
    });
    let actions = Probe::of(&probe.actions, 1);
    assert!(actions.contains(&(outbreak, 5)), "the scheduled outbreak: {actions:?}");
    assert_eq!(
        actions.len(),
        2,
        "the scheduled outbreak and the pressed one: {actions:?}"
    );
    assert_eq!(Probe::of(&probe.params, 1), [(infection, ParamValue::F32(0.3))]);
}

#[test]
fn offload_and_quit_do_not_wait_for_a_runaway_runner() {
    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, None, Some(StepHold::Step(gate.clone())))) else {
        return;
    };
    build_and_step(&mut app, &probe);

    let start = Instant::now();
    app.offload_simulation();
    assert!(
        start.elapsed() < STOP_WAIT + SLACK,
        "Offload waited {:?}",
        start.elapsed()
    );
    assert!(app.sim_thread.is_none() && app.loaded_id().is_none());
    assert_eq!(app.stopping_status(), Some(STILL_STOPPING));

    let quitting_probe = Arc::new(Probe::default());
    let Some(mut quitting) = app_over(held(&sir(), &quitting_probe, None, Some(StepHold::Step(gate.clone())))) else {
        return;
    };
    build_and_step(&mut quitting, &quitting_probe);
    let start = Instant::now();
    drop(quitting);
    assert!(
        start.elapsed() < STOP_WAIT + SLACK,
        "quitting waited {:?}",
        start.elapsed()
    );

    gate.open();
    wait_until("the end of the detached run", || stopped(&mut app));
    wait_until("the end of the run the quit detached", || {
        quitting_probe.dropped.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn a_gpu_build_on_the_build_thread_reports_a_validation_error_as_a_fault() {
    let gpu_life = henad_models::example_models()
        .get("gpu_game_of_life")
        .cloned()
        .expect("the example models include gpu_game_of_life");
    let name = gpu_life.name().to_owned();
    // A buffer past the device's limit, created on the thread the build runs on and outside the entry's own scopes.
    let invalid = gpu_life.wrap_factory(|inner| {
        Arc::new(
            move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| -> Result<ModelState, Fault> {
                if let Some(ctx) = gpu {
                    drop(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("henad_app_test_oversized"),
                        size: ctx.device.limits().max_buffer_size + 4096,
                        usage: wgpu::BufferUsages::STORAGE,
                        mapped_at_creation: false,
                    }));
                }
                inner(params, seed, gpu)
            },
        )
    });
    let Some(mut app) = AppState::headless(set_of(invalid), true) else {
        return;
    };
    app.reset_simulation();
    assert!(app.is_building());
    app.wait_for_build();

    let shown = app.fault.as_ref().expect("the build failed");
    assert_eq!(shown.fault.during, BUILDING);
    assert!(
        matches!(shown.fault.kind, FaultKind::Device(_)),
        "a device fault, got {:?}",
        shown.fault
    );
    assert_eq!(shown.model.as_deref(), Some(name.as_str()));
    assert!(app.sim_thread.is_none() && app.loaded_id().is_none());
    assert!(
        app.render_ctx.faults.take().is_none(),
        "the error escaped the build's scopes to the device's handler"
    );
}

#[test]
fn a_sweep_after_a_detach_probes_in_the_fresh_pool() {
    let probe = Arc::new(Probe::default());
    let Some(mut app) = app_over(held(&sir(), &probe, None, None)) else {
        return;
    };
    // A run past the timeout retires the global pool.
    app.model_pool().retire(0);
    assert_eq!(pool_of(&app), (1, false), "a fresh pool");

    let mut session =
        SweepSession::start(&mut app, sir_spec(), SessionExecution::default(), None).expect("the sweep starts");
    let mut results = ResultsPanel::default();
    wait_until("the end of the sweep", || {
        session.update(0.0, &mut results);
        !session.is_running()
    });
    assert_eq!(
        session.state(&session.progress()),
        SessionState::Finished,
        "the sweep failed: {:?}",
        session.failure
    );
    let probe_threads = Probe::of(&probe.build_threads, 1);
    assert!(probe_threads.iter().all(in_fresh_pool), "probed on {probe_threads:?}");
    let run_threads = Probe::of(&probe.build_threads, 2);
    assert!(
        run_threads
            .iter()
            .all(|name| name.as_deref().is_some_and(|name| name.starts_with("henad-lane-"))),
        "the run builds in its lane, on {run_threads:?}"
    );
}

#[test]
fn quitting_does_not_wait_for_a_sweep_held_in_its_model() {
    let probe = Arc::new(Probe::default());
    let gate = Gate::default();
    let _release = OpenOnDrop(gate.clone());
    let Some(mut app) = app_over(held(&sir(), &probe, Some(gate.clone()), None)) else {
        return;
    };
    let session =
        SweepSession::start(&mut app, sir_spec(), SessionExecution::default(), None).expect("the sweep starts");
    app.sweep.session = Some(session);
    wait_until("the start of the probe", || probe.builds.load(Ordering::SeqCst) == 1);

    let start = Instant::now();
    drop(app);
    assert!(
        start.elapsed() < STOP_WAIT + DEFAULT_STOP_TIMEOUT + SLACK,
        "quitting waited {:?}",
        start.elapsed()
    );

    gate.open();
    wait_until("the end of the probe the quit detached", || {
        probe.built.lock().expect("the probe").contains(&1)
    });
}
