//! Benchmarks that time repetitions of one model's step loop, reporting each repetition as it finishes. Native only.
//!
//! Each repetition builds a fresh state, steps `warmup` ticks untimed, then times `steps` ticks of nothing but the
//! model's step. A CPU repetition steps its warm-up inside one `rayon::scope` on the caller's pool, and its timed
//! steps inside another. A GPU repetition waits for the uploads its build queued, then submits its steps in batches
//! and times them up to the wait that drains the device.
//!
//! Actions fire under [`BENCH_FIRE`], before the step that leaves their tick. An action due from the end of warm-up
//! up to, but not including, the tick a repetition stops on is timed with the steps, and one due on that tick fires
//! after the timer stops.

use std::time::{Duration, Instant};

use henad_compute::entry::ModelState;
use henad_compute::fault::{Fault, STEPPING, catching};
use henad_compute::gpu::fault::catching_on;
use henad_compute::gpu::{GpuContext, GpuSimState, stepping};
use henad_compute::simulation::RunSetup;
use henad_core::action::{Fire, RefusedActions};
use henad_core::metadata::{Backend, Structure};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};

/// Rule under which a repetition's warm-up and timed steps fire actions.
pub const BENCH_FIRE: Fire = Fire::BeforeStep;

/// Model, setup and lengths of one benchmark.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BenchmarkSettings {
    /// Values, base seed and scheduled actions of every repetition.
    pub setup: RunSetup,
    /// Timed steps per repetition.
    pub steps: u64,
    /// Untimed steps before each repetition's timed ones, on that repetition's own state.
    ///
    /// Note that a GPU repetition with no warm-up times wgpu's zero-fill of every buffer its build left unseeded,
    /// which runs in the first submission that touches the buffer. A warm-up of one step keeps it out of the timed
    /// steps.
    pub warmup: u64,
    /// Untimed steps of one throwaway state before the first repetition, to bring the hardware off its idle clocks.
    pub global_warmup: u64,
    /// Number of timed repetitions, each on a freshly built state.
    pub repetitions: u64,
}

impl BenchmarkSettings {
    /// Returns settings that time `steps` steps of `setup` once, with no warm-up.
    pub fn new(setup: RunSetup, steps: u64) -> Self {
        Self {
            setup,
            steps,
            warmup: 0,
            global_warmup: 0,
            repetitions: 1,
        }
    }
}

/// One event of a running benchmark, in the order they happen.
#[derive(Debug)]
#[non_exhaustive]
pub enum BenchmarkEvent<'a> {
    /// Reported before any repetition, after a probe build of a CPU model. A GPU model is not probed.
    Started {
        backend: Backend,
        parallel_jobs: Option<usize>,
    },
    /// The global warm-up is about to build its state and run its steps.
    GlobalWarmupStarted,
    /// The global warm-up ran its steps in this time.
    GlobalWarmupFinished(Duration),
    /// The repetition at this position, from 0, is about to build its state, run its warm-up and time its steps.
    RepetitionStarted(u64),
    /// A repetition ended, its timed steps and the actions due on its last tick included.
    RepetitionFinished(&'a RepetitionReport),
}

/// One timed repetition.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RepetitionReport {
    /// Position of the repetition, from 0.
    pub index: u64,
    /// Seed of the repetition's build. `None` for the model's default seed, as [`RunSetup::seed`] reads.
    pub seed: Option<u64>,
    /// Time the timed steps took.
    pub elapsed: Duration,
    /// Population once the warm-up ended, the denominator of updates per second. A grid model's population is its
    /// cell count.
    pub population_after_warmup: u64,
    /// Population once the timed steps and the actions due on the last tick ended.
    pub population_after_steps: u64,
    /// Heap the state held after warm-up. `None` for a GPU model, whose state lives on the device.
    pub heap_bytes: Option<usize>,
}

/// Timings of every repetition, and what the benchmark measured.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BenchmarkReport {
    /// Every repetition, in the order they ran.
    pub repetitions: Vec<RepetitionReport>,
    /// Number of jobs one step of the probe build split into. `None` for a GPU model.
    pub parallel_jobs: Option<usize>,
    /// The live state's grid on the CPU. On the GPU, the values of a grid model's parameters named `grid_width` and
    /// `grid_height`, the names Henad's models and the template use. `None` for a model without a grid, and for a GPU
    /// grid model that names its size otherwise.
    pub grid_size: Option<(u32, u32)>,
}

/// Times `settings.repetitions` runs of `settings.steps` steps, reporting each repetition as it finishes.
///
/// Repetition `i` builds with the seed `base + i`, wrapping, where `base` is the setup's seed. A setup on the
/// default seed runs every repetition on the default seed. `on_event` runs on the calling thread between repetitions,
/// outside the timed steps.
///
/// A tick-0 action fires once in each repetition, before its first step. Without the template's profile block, a
/// debug build runs the kernels at opt-level 0 in the crate that registers the model, or in henad-models for an
/// example entry, and only a `--release` build gives timings worth comparing.
///
/// # Errors
///
/// Returns a [`Fault`] when a build fails, the model panics, the device reports an error or the model refuses a
/// scheduled action. A GPU model handed no device fails its first build.
pub fn run_benchmark(
    settings: &BenchmarkSettings,
    gpu: Option<&GpuContext>,
    on_event: &mut dyn FnMut(BenchmarkEvent<'_>),
) -> Result<BenchmarkReport, Fault> {
    let setup = &settings.setup;
    let entry = setup.entry();
    let (backend, parallel_jobs) = match (entry.metadata().backend, gpu) {
        // A GPU state reports no jobs. Its probe would hold a whole state on the device into the next build.
        (Backend::Gpu, Some(_)) => (Backend::Gpu, None),
        _ => match entry.build(setup.values(), setup.seed(), gpu)? {
            ModelState::Gpu(_) => (Backend::Gpu, None),
            ModelState::Cpu(state) => (Backend::Cpu, state.parallel_jobs()),
        },
    };
    on_event(BenchmarkEvent::Started { backend, parallel_jobs });

    if settings.global_warmup > 0 {
        on_event(BenchmarkEvent::GlobalWarmupStarted);
        let elapsed = match entry.build(setup.values(), setup.seed(), gpu)? {
            ModelState::Cpu(mut state) => catching(STEPPING, || {
                let start = Instant::now();
                rayon::scope(|_| {
                    for _ in 0..settings.global_warmup {
                        state.step();
                    }
                });
                start.elapsed()
            })?,
            ModelState::Gpu(mut state) => {
                let ctx = device(gpu)?;
                catching_on(ctx, STEPPING, || {
                    let start = Instant::now();
                    stepping::run_steps(&mut *state, ctx, settings.global_warmup)?;
                    Ok(start.elapsed())
                })??
            }
        };
        on_event(BenchmarkEvent::GlobalWarmupFinished(elapsed));
    }

    // Grown as repetitions finish. A count large enough to run until interrupted would abort a reservation up front.
    let mut repetitions = Vec::new();
    let mut grid_size = None;
    for index in 0..settings.repetitions {
        on_event(BenchmarkEvent::RepetitionStarted(index));
        let seed = setup.seed().map(|seed| seed.wrapping_add(index));
        let report = match entry.build(setup.values(), seed, gpu)? {
            ModelState::Cpu(mut state) => {
                let (report, grid) = catching(STEPPING, || cpu_repetition(&mut *state, settings, index, seed))??;
                grid_size = grid;
                report
            }
            ModelState::Gpu(mut state) => {
                let ctx = device(gpu)?;
                catching_on(ctx, STEPPING, || {
                    gpu_repetition(&mut *state, ctx, settings, index, seed)
                })??
            }
        };
        on_event(BenchmarkEvent::RepetitionFinished(&report));
        repetitions.push(report);
    }
    if matches!(entry.metadata().structure, Structure::GpuGrid { .. }) {
        grid_size = grid_size_from_params(entry.param_descriptors(), setup.values());
    }
    Ok(BenchmarkReport {
        repetitions,
        parallel_jobs,
        grid_size,
    })
}

/// Returns the device a GPU state was built on.
fn device(gpu: Option<&GpuContext>) -> Result<&GpuContext, Fault> {
    gpu.ok_or_else(|| Fault::refused(STEPPING, "a GPU model was built with no device"))
}

/// Runs one CPU repetition on `state`, and returns its report and the grid the state holds after it.
///
/// Timing wraps only the step loop, so building and warm-up allocation stay out of the measured window.
fn cpu_repetition(
    state: &mut dyn SimState,
    settings: &BenchmarkSettings,
    index: u64,
    seed: Option<u64>,
) -> Result<(RepetitionReport, Option<(u32, u32)>), Fault> {
    let schedule = settings.setup.schedule();
    let mut refusals = Refusals::default();
    // Stepped from inside the pool. A caller outside it would otherwise inject every parallel pass a kernel runs, and
    // park until it finishes. One inject per loop replaces one per pass per step.
    rayon::scope(|_| {
        for _ in 0..settings.warmup {
            refusals.note(&schedule.run_due(state));
            state.step();
        }
    });
    // A grid model's population is its cell count, and an agent model's its agent count. Either way it is the right
    // denominator for updates per second.
    let population_after_warmup = state.population();
    let heap_bytes = state.heap_bytes();
    let grid = state.grid_view().map(|grid| (grid.width, grid.height));

    let start = Instant::now();
    rayon::scope(|_| {
        for _ in 0..settings.steps {
            refusals.note(&schedule.run_due(state));
            state.step();
        }
    });
    let elapsed = start.elapsed();
    // Outside the timer, so an action on the last tick still lands without being measured.
    refusals.note(&schedule.run_due(state));
    refusals.check()?;
    let report = RepetitionReport {
        index,
        seed,
        elapsed,
        population_after_warmup,
        population_after_steps: state.population(),
        heap_bytes: Some(heap_bytes),
    };
    Ok((report, grid))
}

/// Runs one GPU repetition on `state`, `warmup` untimed steps and then `steps` timed ones, both under [`BENCH_FIRE`].
///
/// An action due on the tick the repetition stops on fires after the timer stops, and is waited for there. Otherwise
/// its work would land in the next repetition, inside the timer when that one has no warm-up, and a fault it raised
/// would be reported late or not at all.
fn gpu_repetition(
    state: &mut dyn GpuSimState,
    ctx: &GpuContext,
    settings: &BenchmarkSettings,
    index: u64,
    seed: Option<u64>,
) -> Result<RepetitionReport, Fault> {
    let schedule = settings.setup.schedule();
    let mut refusals = Refusals::default();
    refusals.note(&stepping::run_steps_acting(
        state,
        ctx,
        settings.warmup,
        schedule,
        BENCH_FIRE,
    )?);
    let population_after_warmup = state.population();
    // A repetition with no warm-up would otherwise time the uploads its build queued.
    ctx.queue.submit([]);
    stepping::wait(ctx)?;

    let start = Instant::now();
    let refused = stepping::run_steps_acting(state, ctx, settings.steps, schedule, BENCH_FIRE)?;
    let elapsed = start.elapsed();
    refusals.note(&refused);
    refusals.note(&stepping::run_due(state, ctx, schedule));
    stepping::wait(ctx)?;
    refusals.check()?;
    Ok(RepetitionReport {
        index,
        seed,
        elapsed,
        population_after_warmup,
        population_after_steps: state.population(),
        heap_bytes: None,
    })
}

/// First scheduled action a state refused, kept until the repetition ends.
///
/// A setup checks every action id before it stores an entry, and an engine refuses only an index past its model's
/// actions. A refusal is therefore an engine contract violation.
#[derive(Default)]
struct Refusals {
    first: Option<(String, u64)>,
}

impl Refusals {
    #[inline]
    fn note(&mut self, refused: &RefusedActions<'_>) {
        if let Some(action) = refused.first()
            && self.first.is_none()
        {
            self.first = Some((action.id.clone(), action.tick));
        }
    }

    fn check(self) -> Result<(), Fault> {
        match self.first {
            None => Ok(()),
            Some((id, tick)) => Err(Fault::refused(
                STEPPING,
                format!("the model refused its own action '{id}' at tick {tick}"),
            )),
        }
    }
}

/// Returns the grid a GPU grid model was built with, read from its parameters named `grid_width` and `grid_height`.
///
/// A GPU state exposes no grid view. Nothing is prepended to a GPU model's parameters, and the two names are the
/// convention Henad's models and the template follow. A model that names its size otherwise has none here.
fn grid_size_from_params(descriptors: &[ParamDescriptor], values: &[ParamValue]) -> Option<(u32, u32)> {
    let find = |id: &str| -> Option<u32> {
        let index = descriptors.iter().position(|descriptor| descriptor.id == id)?;
        match values.get(index) {
            Some(ParamValue::U32(value)) => Some(*value),
            _ => None,
        }
    };
    Some((find("grid_width")?, find("grid_height")?))
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use henad_compute::entry::{ModelState, register_grid_model};
    use henad_compute::gpu::{GpuContext, stepping};
    use henad_core::action::{ActionDescriptor, Fire, Schedule, Scheduled};
    use henad_core::authoring::model::grid_model::GridModel;
    use henad_core::grid::Grid2D;
    use henad_core::metadata::Backend;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::topology::NeighborhoodKind;
    use henad_core::view::{StatDescriptor, StatValue};

    use super::{BENCH_FIRE, BenchmarkEvent, BenchmarkSettings, gpu_repetition, run_benchmark};
    use crate::tests::support::{entry, headless_device};

    /// Ticks at which [`Counter`]'s action saw the grid, in the order it fired.
    static SEEN: Mutex<Vec<u64>> = Mutex::new(Vec::new());

    /// Counts every cell up by one a step, so each cell holds the tick, and records that count when its action runs.
    struct Counter;

    impl GridModel for Counter {
        const NAME: &'static str = "Counter";
        const ID: &'static str = "counter";
        const DESCRIPTION: &'static str = "A model that records the tick its action sees, registered only by tests";
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF]];
        const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
        const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Count", [0xFF, 0xFF, 0xFF, 0xFF])];
        const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor::new("record", "Record")];
        type Params = ();

        fn param_descriptors() -> Vec<ParamDescriptor> {
            Vec::new()
        }

        fn from_params(_params: &[ParamValue]) {}

        fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {
            grid.current_mut().fill(0);
        }

        fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
            cell.wrapping_add(1)
        }

        fn act(_action: usize, grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {
            SEEN.lock()
                .expect("no test panicked holding it")
                .push(u64::from(grid.current()[0]));
        }

        fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
            vec![StatValue::Scalar(f64::from(grid.current()[0]))]
        }
    }

    /// Checks that each repetition fires each action once, before the step that leaves its tick, and the action on
    /// the tick a repetition stops on after the timed steps. A tick-0 action fires once.
    ///
    /// The model records the tick each action sees, so no clock is read.
    #[test]
    fn a_benchmark_fires_each_action_before_its_step() {
        let counter = register_grid_model::<Counter>();
        let mut setup = counter
            .setup()
            .set("grid_width", 4u32)
            .and_then(|setup| setup.set("grid_height", 4u32))
            .expect("the grid fits")
            .with_seed(40);
        for tick in [0, 2, 3, 5, 9] {
            setup = setup.act_at("record", tick).expect("the model declares it");
        }
        let mut settings = BenchmarkSettings::new(setup, 3);
        settings.warmup = 2;
        settings.global_warmup = 4;
        settings.repetitions = 3;
        SEEN.lock().expect("no test panicked holding it").clear();
        let mut events = Vec::new();
        let report = run_benchmark(&settings, None, &mut |event| {
            events.push(match event {
                BenchmarkEvent::Started { .. } => "started".to_owned(),
                BenchmarkEvent::GlobalWarmupStarted => "warming up".to_owned(),
                BenchmarkEvent::GlobalWarmupFinished(_) => "warmed up".to_owned(),
                BenchmarkEvent::RepetitionStarted(index) => format!("rep {index} starts"),
                BenchmarkEvent::RepetitionFinished(repetition) => format!("rep {}", repetition.index),
            });
        })
        .expect("the benchmark runs");

        let seen = SEEN.lock().expect("no test panicked holding it").clone();
        assert_eq!(
            seen,
            [0, 2, 3, 5].repeat(3),
            "warm-up fires 0, the timed steps 2 and 3, the end 5"
        );
        assert_eq!(
            events,
            [
                "started",
                "warming up",
                "warmed up",
                "rep 0 starts",
                "rep 0",
                "rep 1 starts",
                "rep 1",
                "rep 2 starts",
                "rep 2"
            ]
        );
        let seeds: Vec<Option<u64>> = report.repetitions.iter().map(|repetition| repetition.seed).collect();
        assert_eq!(seeds, [Some(40), Some(41), Some(42)], "repetition i takes base + i");
        assert_eq!(report.grid_size, Some((4, 4)));

        let mut default_seed = BenchmarkSettings::new(counter.setup(), 1);
        default_seed.repetitions = 2;
        let report = run_benchmark(&default_seed, None, &mut |_| {}).expect("the benchmark runs");
        assert!(report.repetitions.iter().all(|repetition| repetition.seed.is_none()));
    }

    /// Checks that a repetition count large enough to run until interrupted reports its first repetition. A
    /// reservation for every report up front would abort the process before it.
    #[test]
    fn a_benchmark_of_endless_repetitions_reports_the_first() {
        let setup = register_grid_model::<Counter>()
            .setup()
            .set("grid_width", 4u32)
            .and_then(|setup| setup.set("grid_height", 4u32))
            .expect("the grid fits");
        let mut settings = BenchmarkSettings::new(setup, 1);
        settings.repetitions = 1 << 56;
        let mut reported = 0;
        let interrupted = std::panic::catch_unwind(AssertUnwindSafe(|| {
            run_benchmark(&settings, None, &mut |event| {
                if let BenchmarkEvent::RepetitionFinished(_) = event {
                    reported += 1;
                    panic!("the host interrupts the benchmark");
                }
            })
        }));
        assert!(interrupted.is_err());
        assert_eq!(reported, 1);
    }

    /// Checks that a GPU benchmark builds one state per repetition and no probe, and that a GPU model handed no
    /// device fails before it starts.
    #[test]
    fn a_gpu_benchmark_builds_no_probe() {
        let Some(ctx) = headless_device() else {
            return;
        };
        let builds = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&builds);
        let sir = entry("gpu_sir", Some(&ctx)).wrap_factory(move |create| {
            Arc::new(
                move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| {
                    counted.fetch_add(1, Ordering::Relaxed);
                    create(params, seed, gpu)
                },
            )
        });
        let setup = sir
            .setup()
            .set("grid_width", 32u32)
            .and_then(|setup| setup.set("grid_height", 32u32))
            .expect("the grid fits");
        let mut settings = BenchmarkSettings::new(setup, 2);
        settings.repetitions = 2;
        let mut started = Vec::new();
        let mut on_event = |event: BenchmarkEvent<'_>| {
            if let BenchmarkEvent::Started { backend, parallel_jobs } = event {
                started.push((backend, parallel_jobs));
            }
        };
        let report = run_benchmark(&settings, Some(&ctx), &mut on_event).expect("the benchmark runs");
        assert_eq!(builds.load(Ordering::Relaxed), 2, "one build per repetition");
        assert_eq!(report.grid_size, Some((32, 32)));
        assert!(run_benchmark(&settings, None, &mut on_event).is_err());
        assert_eq!(started, [(Backend::Gpu, None)], "the run with no device never starts");
    }

    fn schedule_at(ticks: &[u64]) -> Schedule {
        let entries = ticks
            .iter()
            .map(|&tick| Scheduled {
                index: 0,
                id: "act".to_owned(),
                tick,
            })
            .collect();
        Schedule::from_entries(entries)
    }

    /// Returns the ticks each run fires when runs of `counts` steps go back to back from tick 0.
    fn fire_runs(schedule: &Schedule, counts: &[u64], fire: Fire) -> Vec<Vec<u64>> {
        let mut start = 0;
        counts
            .iter()
            .map(|&count| {
                let fired = schedule.fire_ticks(start, count, fire);
                start += count;
                fired
            })
            .collect()
    }

    /// Checks that back-to-back runs under [`BENCH_FIRE`] fire the ticks the CPU loop fires, leaving the end tick to
    /// the caller.
    ///
    /// A GPU repetition is a warm-up run and a timed run under that rule, and fires the tick it stops on after the
    /// timer. The test reads the rule alone, and times nothing.
    #[test]
    fn a_timed_run_fires_the_ticks_the_cpu_loop_times() {
        let schedule = schedule_at(&(0..=8).collect::<Vec<u64>>());
        for warmup in 0..4 {
            for steps in 0..4 {
                let runs = fire_runs(&schedule, &[warmup, steps], BENCH_FIRE);
                let expected: [Vec<u64>; 2] = [(0..warmup).collect(), (warmup..warmup + steps).collect()];
                assert_eq!(runs, expected, "--warmup {warmup} --steps {steps}");
            }
        }
    }

    /// Checks that a GPU repetition fires each action once and on its own tick, the tick it stops on included.
    ///
    /// Its counts have to match a simulation stepped over the same ticks. `seed_outbreak` infects a share of the
    /// cells still susceptible, so a press missed, repeated or moved to another tick changes the counts.
    #[test]
    fn a_gpu_benchmark_repetition_fires_every_action_once() {
        let Some(ctx) = headless_device() else {
            return;
        };
        let sir = entry("gpu_sir", Some(&ctx));
        let mut setup = sir
            .setup()
            .set("grid_width", 64u32)
            .and_then(|setup| setup.set("grid_height", 64u32))
            .expect("the grid fits")
            .with_seed(1);
        for tick in [0, 2, 3, 5] {
            setup = setup.act_at("seed_outbreak", tick).expect("gpu_sir declares it");
        }
        let counts = |entries: &[henad_core::view::StatEntry]| -> Vec<f64> {
            entries.iter().map(|entry| entry.value.scalar()).collect()
        };
        for (warmup, steps) in [(0, 0), (0, 3), (2, 0), (2, 3)] {
            let mut settings = BenchmarkSettings::new(setup.clone(), steps);
            settings.warmup = warmup;
            let Ok(ModelState::Gpu(mut state)) = sir.build(setup.values(), setup.seed(), Some(&ctx)) else {
                panic!("gpu_sir builds as a GPU model");
            };
            gpu_repetition(&mut *state, &ctx, &settings, 0, setup.seed()).expect("a repetition");
            let after_repetition = counts(&stepping::sample_stats(&mut *state, &ctx).expect("a sample"));
            let mut simulation = setup.build(Some(&ctx)).expect("gpu_sir builds");
            simulation.run_to(warmup + steps).expect("gpu_sir steps");
            let stepped = counts(simulation.stats().expect("a sample").entries());
            assert_eq!(after_repetition, stepped, "--warmup {warmup} --steps {steps}");
        }
    }
}
