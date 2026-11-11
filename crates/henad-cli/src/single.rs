//! One configuration of a model, run as a benchmark, a stats export or a final state export.

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use numfmt::{Formatter, Scales};

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_compute::runtime_info::RuntimeInfo;
use henad_compute::simulation::{RunSetup, Simulation};
use henad_core::action::Schedule;
use henad_core::explore::value::{ValueError, resolve_params};
use henad_core::export::StatsWriter;
use henad_core::metadata::Backend;
use henad_explore::benchmark::{BenchmarkEvent, BenchmarkReport, BenchmarkSettings, run_benchmark};
use henad_explore::model_file::ModelFile;

use crate::{Args, json_report};

/// Work that one configuration of a model runs, outside a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleRun<'a> {
    /// `--export-stats PATH`. Writes the stat time series to the path.
    ExportStats(&'a Path),
    /// `--export PATH`. Writes the final state to the path.
    ExportFinal(&'a Path),
    /// Times the steps of each repetition.
    Benchmark,
}

/// Runs one configuration of `entry` for `--export-stats`, `--export` or a benchmark, as `run` says.
///
/// `file` is the model file that `entry` came from, if any. A sheet's work past a budget is then named by its lines.
pub(crate) fn run_single(
    entry: &ModelEntry,
    file: Option<&ModelFile>,
    args: &Args,
    run: SingleRun<'_>,
    gpu_ctx: Option<&GpuContext>,
    runtime: Option<&RuntimeInfo>,
) -> Result<()> {
    let params = resolve_params(entry.param_descriptors(), &args.set).map_err(set_error)?;
    let schedule = Schedule::parse(&args.act, entry.id(), entry.action_descriptors())?;
    if let Some(last) = schedule.last_tick()
        && last > args.warmup + args.steps
    {
        eprintln!(
            "note: --act at tick {last} is past the {} this run reaches, so it never fires",
            args.warmup + args.steps
        );
    }

    // The limits are checked ahead of the factory. An oversized run is then rejected with an error that lists its
    // shortfalls, instead of failing at whichever binding the device happened to reject first.
    if let Some(ctx) = gpu_ctx {
        let limits = ctx.device.limits();
        let shortfalls = match file.and_then(ModelFile::sheet) {
            Some(sheet) => sheet.shortfalls(entry, &params, &limits),
            None => entry.shortfalls(&params, &limits),
        };
        if !shortfalls.is_empty() {
            bail!("'{}' does not fit this device: {}", entry.id(), shortfalls.join("; "));
        }
    }
    let setup = RunSetup::from_parts(entry, &params, args.seed, schedule)?;

    match run {
        SingleRun::ExportStats(path) => export_stats(&setup, args, path, gpu_ctx),
        SingleRun::ExportFinal(path) => export_final(&setup, args, path),
        SingleRun::Benchmark => {
            let adapter = runtime.map(|r| r.adapter.name.as_str());
            benchmark(setup, args, gpu_ctx, adapter)
        }
    }
}

/// Runs the benchmark for `setup`, printing each repetition as it finishes and the result at the end.
///
/// With `--json` the lines follow `benchmarks/protocol.md`: the `info` line before any repetition, a `rep` line as
/// each one finishes, then the `summary`. The human report goes to stdout and the progress log to stderr.
fn benchmark(setup: RunSetup, args: &Args, gpu_ctx: Option<&GpuContext>, adapter: Option<&str>) -> Result<()> {
    let entry = setup.entry().clone();
    let mut settings = BenchmarkSettings::new(setup, args.steps);
    settings.warmup = args.warmup;
    settings.global_warmup = args.global_warmup;
    settings.repetitions = args.reps;
    let mut backend = Backend::Cpu;
    // Whether stderr ends in a repetition's number with no time after it yet.
    let mut number_open = false;
    let finished = run_benchmark(&settings, gpu_ctx, &mut |event| {
        number_open = matches!(
            event,
            BenchmarkEvent::GlobalWarmupStarted | BenchmarkEvent::RepetitionStarted(_)
        );
        match event {
            BenchmarkEvent::Started {
                backend: started,
                parallel_jobs,
            } => {
                backend = started;
                let (variant, adapter, tag) = match started {
                    Backend::Gpu => ("gpu", adapter, " [GPU]"),
                    Backend::Cpu => ("cpu", None, ""),
                };
                if args.json {
                    json_report::info(
                        entry.id(),
                        variant,
                        rayon::current_num_threads(),
                        parallel_jobs,
                        adapter,
                    );
                }
                eprintln!(
                    "benchmarking {} ({}){tag}: {} steps x {} reps, {} warmup, {} global-warmup",
                    entry.name(),
                    entry.id(),
                    args.steps,
                    args.reps,
                    args.warmup,
                    args.global_warmup
                );
                if cfg!(debug_assertions) {
                    eprintln!("!!! warning: debug build; use --release for benchmarking !!!");
                }
            }
            // The number shows while the state builds and steps, and the time follows on the same line.
            BenchmarkEvent::GlobalWarmupStarted => eprint!("  #{: >4}: ", 0),
            BenchmarkEvent::GlobalWarmupFinished(elapsed) => {
                eprintln!("{elapsed:>8.3?}  ({} global warmup steps)", args.global_warmup);
            }
            BenchmarkEvent::RepetitionStarted(index) => eprint!("  #{: >4}: ", index + 1),
            BenchmarkEvent::RepetitionFinished(repetition) => {
                eprintln!("{:>8.3?}", repetition.elapsed);
                let (population, after) = (repetition.population_after_warmup, repetition.population_after_steps);
                // A population at steady state moves by about its square root. A move that small is not worth a note.
                let noise = (3.0 * (population as f64).sqrt()) as u64;
                if backend == Backend::Cpu && after.abs_diff(population) > (population / 10).max(noise) {
                    eprintln!(
                        "  note: the population went from {population} to {after} during the timed steps. \
                     Updates per second are computed from the population after warmup. \
                     A longer --warmup can reach a steady population first."
                    );
                }
                if args.json {
                    json_report::rep(repetition, args.steps, args.warmup);
                }
            }
            // Other events print nothing.
            _ => {}
        }
    });
    // The line of a repetition that failed after its number was printed ends here. Otherwise `Error:` follows the
    // number on the same line.
    if finished.is_err() && number_open {
        eprintln!();
    }
    let timings = Timings::new(&finished?);
    if args.json {
        json_report::summary(&timings, args.steps, &settings.setup);
    } else {
        print_report(&timings, args.steps)?;
    }
    Ok(())
}

/// Timings of a benchmark's repetitions, with the population and grid size that the rates and the report read.
pub(crate) struct Timings {
    /// Wall-clock time of each repetition, in the order they ran. Never empty, since `--reps` is at least 1.
    pub(crate) samples: Vec<Duration>,
    /// Population sampled after the last repetition's warm-up: the total cell count of a grid model, and the agent
    /// count of an agent model.
    pub(crate) population: u64,
    /// Width and height of a grid model.
    pub(crate) grid_size: Option<(u32, u32)>,
}

impl Timings {
    /// Returns the timings of the repetitions of `report`.
    fn new(report: &BenchmarkReport) -> Self {
        Self {
            samples: report.repetitions.iter().map(|repetition| repetition.elapsed).collect(),
            population: report
                .repetitions
                .last()
                .map_or(0, |repetition| repetition.population_after_warmup),
            grid_size: report.grid_size,
        }
    }

    pub(crate) fn min(&self) -> Option<Duration> {
        self.samples.iter().min().copied()
    }

    pub(crate) fn max(&self) -> Option<Duration> {
        self.samples.iter().max().copied()
    }

    /// Returns the median, averaging the middle two on an even count.
    ///
    /// The driver's `statistics.median` computes the same value.
    pub(crate) fn median(&self) -> Option<Duration> {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let middle = sorted.len() / 2;
        match sorted.len() {
            0 => None,
            count if count % 2 == 1 => Some(sorted[middle]),
            _ => Some((sorted[middle - 1] + sorted[middle]) / 2),
        }
    }

    /// Returns the mean as the text report prints it, the `Duration` sum divided by the count, truncated to whole
    /// nanoseconds.
    fn mean(&self) -> Duration {
        self.samples.iter().sum::<Duration>() / (self.samples.len() as u32)
    }

    /// Returns the mean as the `summary` line records it, the `f64` mean of the samples in seconds.
    pub(crate) fn mean_secs(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().map(Duration::as_secs_f64).sum::<f64>() / self.samples.len() as f64
    }

    /// Returns the population standard deviation of the samples around `mean`, in seconds.
    pub(crate) fn std_dev_secs(&self, mean: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let variance = self
            .samples
            .iter()
            .map(|sample| {
                let diff = sample.as_secs_f64() - mean;
                diff * diff
            })
            .sum::<f64>()
            / self.samples.len() as f64;
        variance.sqrt()
    }
}

/// Prints the benchmark result of `timings`, each repetition covering `steps_per_rep` steps.
fn print_report(timings: &Timings, steps_per_rep: u64) -> Result<()> {
    println!("benchmark result:");
    // The samples are never empty, so the defaults are never used.
    let min = timings.min().unwrap_or_default();
    let max = timings.max().unwrap_or_default();
    let mean = timings.mean();
    let median = timings.median().unwrap_or_default();
    let std_dev = Duration::from_secs_f64(timings.std_dev_secs(mean.as_secs_f64()));

    let mut f = Formatter::new()
        .scales(Scales::none())
        .separator(' ')?
        .precision(numfmt::Precision::Decimals(3));

    println!("  min:     {min:>10.3?}");
    println!("  median:  {median:>10.3?}");
    println!("  max:     {max:>10.3?}");
    println!("  mean:    {mean:>10.3?}");
    println!("  std dev: {std_dev:>10.3?}");
    let mean_steps_per_sec = f.fmt2(steps_per_rep as f64 / mean.as_secs_f64());
    println!("  > mean steps/sec:   {mean_steps_per_sec:>20}");
    let mean_updates_per_sec = f.fmt2((steps_per_rep as f64 * timings.population as f64) / mean.as_secs_f64());
    println!("  > mean updates/sec: {mean_updates_per_sec:>20}");
    if let Some((w, h)) = timings.grid_size {
        f = f.precision(numfmt::Precision::Decimals(0));
        let grid_size = f.fmt2(w as u64 * h as u64);
        println!("  > grid size:        {grid_size:>16}");
    }
    Ok(())
}

/// Runs `setup` once for `--warmup` plus `--steps` ticks and writes the final state to `path`.
///
/// Each action fires once, including actions due on the tick that the run ends on.
fn export_final(setup: &RunSetup, args: &Args, path: &Path) -> Result<()> {
    let entry = setup.entry();
    if entry.gpu_needs().is_some() {
        bail!("model '{}' is GPU-backed; this path is CPU-only", entry.id());
    }
    let topology = entry.topology_hint();
    if !topology.grid && !topology.agents {
        bail!("model exposes no CPU-side view to export");
    }
    let mut simulation = setup.build(None)?;
    simulation.run_to(args.warmup + args.steps)?;
    let file = File::create(path).with_context(|| format!("cannot create '{}'", path.display()))?;
    let mut out = BufWriter::new(file);
    simulation.write_state(&mut out)?;
    out.flush()?;
    eprintln!(
        "exported final state (tick {}) to {}",
        simulation.tick(),
        path.display()
    );
    Ok(())
}

/// Runs `setup` once and writes the per-tick stat series to `path` as CSV.
///
/// A model with a time scale adds a `time` column after `tick`, from the setup's tick length.
///
/// Unlike [`benchmark`], this path is untimed. Each sample is significant work, a full reduction over the grid for a
/// `GridModel` and a blocking readback for a GPU model, so a run's time does not measure the cost of its steps.
/// `--reps` is ignored. A time series is one trajectory, and N repetitions would put N trajectories in one file.
fn export_stats(setup: &RunSetup, args: &Args, path: &Path, gpu_ctx: Option<&GpuContext>) -> Result<()> {
    if args.stats_every == 0 {
        bail!("--stats-every must be at least 1");
    }
    if args.reps > 1 {
        eprintln!("note: --reps is ignored by --export-stats");
    }

    let total = args.warmup + args.steps;
    let entry = setup.entry();
    let file = File::create(path).with_context(|| format!("cannot create '{}'", path.display()))?;
    let mut writer = StatsWriter::new(BufWriter::new(file));
    if let Some(tick_length) = entry
        .metadata()
        .time
        .as_ref()
        .and_then(|time| time.tick_length_in(entry.param_descriptors(), setup.values()))
    {
        writer = writer.tick_length(tick_length);
    }

    eprintln!(
        "exporting stats for {} ({}): {} steps, sampling every {}",
        entry.name(),
        entry.id(),
        total,
        args.stats_every
    );

    let mut simulation = setup.build(gpu_ctx)?;
    let rows = write_series(&mut simulation, total, args.stats_every, writer)?;
    eprintln!("wrote {rows} rows to {}", path.display());
    Ok(())
}

/// Steps `simulation` to `total` and writes a sample at its current tick, every `every` ticks and at `total`, then
/// returns the number of rows written.
///
/// The final tick is always written, even off a sampling boundary. The end state of a run is the one value a
/// reader is most likely to want.
pub(crate) fn write_series<W: std::io::Write + Send>(
    simulation: &mut Simulation,
    total: u64,
    every: u64,
    mut writer: StatsWriter<W>,
) -> Result<u64> {
    let flow = simulation.run_sampled(total, every, |sample| {
        match writer.push(sample.tick(), sample.entries()) {
            Ok(()) => ControlFlow::Continue(()),
            Err(error) => ControlFlow::Break(error),
        }
    })?;
    if let ControlFlow::Break(error) = flow {
        return Err(error.into());
    }
    Ok(writer.finish()?)
}

/// Converts an error from resolving `--set` into a command-line error with `--set ID` as its context.
fn set_error(error: ValueError) -> anyhow::Error {
    match error {
        ValueError::Param { id, source } => anyhow::Error::new(*source).context(format!("--set {id}")),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use henad_core::explore::value::{parse_overrides, resolve_params};
    use henad_core::export::stats_csv::StatsWriter;
    use henad_models::example_models;

    use super::write_series;
    use crate::tests::{SmallGpuGrid, cpu_simulation, rows};

    /// Each exported sample is prepared as a publish would be, so a stat computed in `prepare_view` is current.
    /// Team Assembly's component stats are computed there.
    ///
    /// With `p` at zero every tick adds a separate clique of four newcomers, and nothing retires yet.
    /// Rows are written at tick 0, at tick 2 on the sampling boundary, and at the final tick 3 off it.
    #[test]
    fn exported_stats_are_prepared_like_a_publish() {
        let entry = example_models()
            .get("team_assembly")
            .cloned()
            .expect("team_assembly is registered");
        let overrides =
            parse_overrides(&["num_agents=4".to_owned(), "team_size=4".to_owned(), "p=0".to_owned()]).expect("valid");
        let params = resolve_params(entry.param_descriptors(), &overrides).expect("in range");
        let mut simulation = cpu_simulation(&entry, &params, 1, &[]);

        let mut out = Vec::new();
        let rows = write_series(&mut simulation, 3, 2, StatsWriter::new(&mut out)).expect("writes");
        assert_eq!(rows, 3);
        let text = String::from_utf8(out).expect("utf-8");
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().expect("a header").split(',').collect();
        let at = |name: &str| header.iter().position(|h| *h == name).expect("the column is exported");
        let (tick, share, size) = (at("tick"), at("Giant Component Share"), at("Mean Component Size"));
        for (line, cliques) in lines.zip([1.0, 3.0, 4.0]) {
            let row: Vec<f64> = line.split(',').map(|v| v.parse().expect("a number")).collect();
            assert!(
                (row[share] - 1.0 / cliques).abs() < 1e-9,
                "tick {}: giant share {}",
                row[tick],
                row[share]
            );
            assert_eq!(row[size], 4.0, "tick {}", row[tick]);
        }
    }

    /// Checks that a GPU action on a sampling boundary fires once, whatever `--stats-every` is.
    ///
    /// `randomise` draws a fresh board on every press. At `--stats-every 1` tick 2 is a boundary, and a
    /// second press there would change every later row. At `--stats-every 3` it is inside a run.
    #[test]
    fn a_gpu_action_fires_once_whatever_the_sampling_interval() {
        let Some(life) = SmallGpuGrid::new("gpu_game_of_life") else {
            return;
        };
        let schedule = life.schedule(&["randomise@2"]);
        let every_tick = rows(life.simulation(1, &schedule), 1, 6);
        let every_third = rows(life.simulation(1, &schedule), 3, 6);
        assert_eq!(every_tick.len(), 7, "ticks 0 to 6");
        let shared = [0, 3, 6].map(|tick| every_tick[tick].clone());
        assert_eq!(every_third, shared, "rows at ticks 0, 3 and 6");
    }
}
