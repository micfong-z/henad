//! A headless benchmark runner for Henad models.
//!
//! The non-GUI sibling of `henad-app`. It builds a model from the shared [`model_registry`] and
//! steps its `SimState` in a bare loop, with no rendering, no `SimThread` and no pacing, so a
//! measurement times nothing but `state.step()`.
//!
//! Both CPU and GPU models run. GPU support needs a `wgpu::Device`, which `henad-compute` never
//! creates itself, so this binary acquires one headlessly (see [`acquire_headless`]) and hands the
//! resulting [`GpuContext`] to [`model_registry`]. Without a device the registry falls back to
//! CPU-only. GPU stepping does *not* go through `SimState::step()`, which would leave one
//! unwaited submission per step. See [`stepping::run_steps`].
//!
//! ```text
//! henad-cli --list
//! henad-cli ants --params
//! henad-cli game_of_life --steps 10000 --reps 5
//! henad-cli sir --set grid_width=512 --steps 2000 --export final.txt
//! henad-cli sir --steps 2000 --export-stats sir.csv --stats-every 10
//! henad-cli game_of_life --steps 1000 --act clear@500 --export final.txt
//! henad-cli gpu_game_of_life --set grid_width=4096 --set grid_height=4096 --steps 10000
//! henad-cli sir --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 500 --out sir-sweep
//! henad-cli --spec crates/henad-explore/specs/sir_sweep.toml --out sir-sweep
//! henad-cli --merge shard-0 shard-1 --out sir-sweep
//! ```
//!
//! Two export paths, deliberately separate: `--export` writes the *final state* (the grid or point
//! cloud at the end of the run), `--export-stats` writes the *time series* (one row per sampled
//! tick). Both formats live in `henad_core::export`, which the app writes through too.
//!
//! `--out`, `--spec` or `--dry-run` runs a sweep instead, many runs over a grid of parameter values written to a
//! directory, and `--merge` joins the directories of a sweep's shards. See [`explore`].

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "henad-cli is a command-line tool: stdout carries its results, stderr its progress log"
)]

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use clap::{ArgGroup, Parser};

use henad_compute::fault::install_panic_hook;
use henad_compute::gpu::{GpuContext, GpuSimState, stepping};
use henad_compute::runtime_info::{GpuVerdict, HostInfo, RuntimeInfo, classify_adapter};
use henad_core::action::{Fire, Schedule};
use henad_core::explore::value::{ValueError, parse_overrides, resolve_params};
use henad_core::export::{StatsWriter, state as state_export};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamFormat, ParamKind, ParamValue};
use henad_explore::device::acquire_headless;
use henad_models::registry::{ModelEntry, ModelState, model_registry};

use crate::actions::{BENCH_FIRE, note_refused};
use crate::explore::{ExploreArgs, LoadedSpec};
use numfmt::{Formatter, Scales};

mod actions;
mod explore;
mod json_report;

/// Headless benchmark runner for Henad models.
#[derive(Parser)]
#[command(name = "henad-cli", version, about)]
#[command(group(
    ArgGroup::new("explore")
        .args(["out", "spec", "dry_run"])
        .multiple(true)
        .conflicts_with_all(["list", "export", "export_stats", "global_warmup"])
))]
struct Args {
    /// Model id to run (see `--list`). Optional with `--spec`.
    #[arg(required_unless_present_any = ["list", "info", "spec", "merge"])]
    model: Option<String>,

    /// Steps to run (and time) per rep, or per run of a sweep.
    #[arg(long, default_value_t = 1000)]
    steps: u64,

    /// Untimed steps run before each timed rep, on that rep's own state, to reach a steady sim regime.
    #[arg(long, default_value_t = 0)]
    warmup: u64,

    /// Untimed steps for a one-time hardware warm-up before the timed reps. Ramps GPU clocks and
    /// pays first-use compilation, so rep 1 isn't cold. Its cost scales with the workload.
    #[arg(long = "global-warmup", default_value_t = 0)]
    global_warmup: u64,

    /// RNG seed used. A sweep derives each run's seed from it and uses 0 by default.
    #[arg(long)]
    seed: Option<u64>,

    /// Independent timed runs to collect, each on a freshly created state. A sweep runs this many replicates of
    /// each config.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
    reps: u64,

    /// Override a model parameter, e.g. `--set grid_size=512`. Repeatable.
    #[arg(long = "set", value_name = "ID=VALUE")]
    set: Vec<String>,

    /// Run one of the model's actions at a tick, e.g. `--act clear@500`. Repeatable. An unknown id is
    /// refused, and the error lists the ids the model declares. Each rep and each run of a sweep replays the same
    /// schedule. A sweep names the action by its id, or `ID_2` for the second `--act` of an id, skipping a name an
    /// earlier `--act` has taken.
    #[arg(long = "act", value_name = "ID@TICK")]
    act: Vec<String>,

    /// Write the final state (after warmup + steps) to this path, then exit.
    #[arg(long, value_name = "PATH")]
    export: Option<PathBuf>,

    /// Write the per-tick stat time series to this path as CSV, then exit. Runs one configuration
    /// for `warmup + steps` steps, sampling every `--stats-every` ticks.
    #[arg(long = "export-stats", value_name = "PATH")]
    export_stats: Option<PathBuf>,

    /// Sample stats every N ticks when using `--export-stats` or sweeping. 1 records every tick.
    #[arg(long = "stats-every", default_value_t = 1, value_name = "N")]
    stats_every: u64,

    /// List available models and exit.
    #[arg(long)]
    list: bool,

    /// Print the model's parameters (the ids `--set` takes, with kinds and defaults) and exit. With `--json`,
    /// print them as one JSON object with the model's stats and actions.
    #[arg(long, conflicts_with_all = ["out", "dry_run"])]
    params: bool,

    /// Print host and GPU information. With no model and no sweep it prints and exits. Otherwise it
    /// prints as a provenance header before the benchmark or sweep.
    #[arg(long)]
    info: bool,

    /// Emit one JSON object per line instead of the human report, for a driver to parse.
    #[arg(long)]
    json: bool,

    /// Worker threads for CPU models. 0 leaves rayon's own choice, which is one per logical cpu.
    #[arg(long, default_value_t = 0, value_name = "N")]
    threads: usize,

    #[command(flatten)]
    explore: ExploreArgs,
}

/// Work one command line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    /// `--merge`. Joins the directories of a sweep's shards, with no model and no device.
    Merge,
    /// `--info` with no model and no sweep. Prints the runtime and exits.
    InfoOnly,
    Params,
    Explore,
    ExportStats,
    ExportFinal,
    Benchmark,
}

impl Mode {
    /// Returns the mode of `args`, taking the first that applies in the order the variants are declared.
    fn of(args: &Args) -> Self {
        if args.list {
            Self::List
        } else if !args.explore.merge.is_empty() {
            Self::Merge
        } else if args.info && args.model.is_none() && !args.explore.is_sweep() {
            Self::InfoOnly
        } else if args.params {
            Self::Params
        } else if args.explore.is_sweep() {
            Self::Explore
        } else if args.export_stats.is_some() {
            Self::ExportStats
        } else if args.export.is_some() {
            Self::ExportFinal
        } else {
            Self::Benchmark
        }
    }
}

fn main() -> Result<ExitCode> {
    install_panic_hook();

    let args = Args::parse();
    let mode = Mode::of(&args);

    // Before anything builds a state. Rayon's global pool is set once per process, and
    // `HostInfo::worker_threads` reads back whatever it ends up with.
    if args.threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.threads)
            .build_global()
            .context("cannot size the worker pool")?;
    }
    if mode == Mode::Merge {
        return explore::merge_shards(&args);
    }

    // Best-effort headless GPU: acquire a device so GPU models can be listed and run. If none is
    // available (e.g. CI with no GPU), fall back to a CPU-only registry rather than failing.
    let (gpu_ctx, runtime) = match acquire_headless() {
        Ok((ctx, runtime)) => (Some(ctx), Some(runtime)),
        Err(err) => {
            eprintln!("note: no GPU available ({err}); GPU models disabled");
            (None, None)
        }
    };

    // `force_fallback_adapter: false` does not stop a software rasteriser (lavapipe, WARP) being
    // returned when it is the only adapter present.
    if let Some(runtime) = &runtime
        && classify_adapter(&runtime.adapter) == GpuVerdict::Absent
    {
        eprintln!(
            "!!! warning: adapter '{}' is a software rasteriser, not a GPU; \
                 GPU-model results from this machine are not GPU results !!!",
            runtime.adapter.name
        );
    }

    if args.info {
        if args.json {
            json_report::runtime(runtime.as_ref());
        } else {
            print_runtime_info(runtime.as_ref());
        }
    }

    let registry = model_registry(gpu_ctx.clone());

    match mode {
        Mode::List => {
            print_models(&registry);
            return Ok(ExitCode::SUCCESS);
        }
        Mode::InfoOnly => return Ok(ExitCode::SUCCESS),
        _ => {}
    }

    let spec = args.explore.spec.as_deref().map(LoadedSpec::load).transpose()?;
    let model_id = match (args.model.as_deref(), spec.as_ref()) {
        (Some(id), Some(spec)) if id != spec.model() => {
            bail!("model '{id}' does not match the spec's model '{}'", spec.model())
        }
        (Some(id), _) => id,
        (None, Some(spec)) => spec.model(),
        (None, None) => bail!("a model id is required (try --list)"),
    };
    let entry = registry
        .iter()
        .find(|e| e.id == model_id)
        .with_context(|| format!("unknown model '{model_id}' (try --list)"))?;

    match mode {
        Mode::Params if args.json => json_report::emit(&json_report::params(entry, gpu_ctx.as_ref())),
        Mode::Params => print!("{}", params_text(entry)),
        Mode::Explore => return explore::run(&args, entry, gpu_ctx.as_ref(), runtime.as_ref(), spec),
        _ => return run_single(entry, &args, mode, gpu_ctx.as_ref(), runtime.as_ref()).map(|()| ExitCode::SUCCESS),
    }
    Ok(ExitCode::SUCCESS)
}

/// Runs one configuration of `entry` for `--export-stats`, `--export` or a benchmark, as `mode` says.
fn run_single(
    entry: &ModelEntry,
    args: &Args,
    mode: Mode,
    gpu_ctx: Option<&GpuContext>,
    runtime: Option<&RuntimeInfo>,
) -> Result<()> {
    let overrides = parse_overrides(&args.set)?;
    let params = resolve_params(&entry.param_descriptors, &overrides).map_err(set_error)?;
    let schedule = Schedule::parse(&args.act, &entry.id, &entry.action_descriptors)?;
    if let Some(last) = schedule.last_tick()
        && last > args.warmup + args.steps
    {
        eprintln!(
            "note: --act at tick {last} is past the {} this run reaches, so it never fires",
            args.warmup + args.steps
        );
    }

    // Ahead of the factory. An over-sized run is then refused by name instead of by whichever
    // binding the device happened to reject first.
    if let Some(ctx) = gpu_ctx {
        let shortfalls = entry.shortfalls(&params, &ctx.device.limits());
        if !shortfalls.is_empty() {
            bail!("'{}' does not fit this device: {}", entry.id, shortfalls.join("; "));
        }
    }

    match (mode, &args.export_stats, &args.export) {
        (Mode::ExportStats, Some(path), _) => export_stats(entry, &params, args, &schedule, path, gpu_ctx),
        (Mode::ExportFinal, _, Some(path)) => export_final(entry, &params, args, &schedule, path),
        _ => {
            let adapter = runtime.map(|r| r.adapter.name.as_str());
            run_benchmark(entry, &params, args, &schedule, gpu_ctx, adapter)
        }
    }
}

/// Benchmark provenance. Goes to stdout with the results, not the progress log.
fn print_runtime_info(runtime: Option<&RuntimeInfo>) {
    let collected;
    let host = if let Some(runtime) = runtime {
        &runtime.host
    } else {
        collected = HostInfo::collect();
        &collected
    };

    let fmt_opt = |value: Option<usize>| value.map_or_else(|| "unknown".to_owned(), |n| n.to_string());

    println!("runtime info:");
    println!("  host:");
    println!("    platform:        {} ({})", host.os, host.arch);
    println!("    logical cpus:    {}", fmt_opt(host.logical_cpus));
    println!("    worker threads:  {}", fmt_opt(host.worker_threads));

    match runtime {
        None => println!("  gpu:               none (GPU models disabled)"),
        Some(runtime) => {
            let adapter = &runtime.adapter;
            println!("  gpu:");
            println!("    adapter:         {}", adapter.name);
            println!("    type:            {:?}", adapter.device_type);
            println!("    backend:         {}", adapter.backend);
            if !adapter.driver_info.is_empty() {
                println!("    driver:          {}", adapter.driver_info);
            }
            let limits = &runtime.granted;
            println!(
                "    max storage binding: {} bytes ({} u32 cells)",
                limits.max_storage_buffer_binding_size,
                limits.max_storage_buffer_binding_size / 4
            );
            println!("    max buffer size:     {} bytes", limits.max_buffer_size);
            println!("    max 2d texture:      {}", limits.max_texture_dimension_2d);
            println!(
                "    storage buffers:     {} per shader stage",
                limits.max_storage_buffers_per_shader_stage
            );
            println!("    display texture cap: {0}x{0}", runtime.display_cap());
        }
    }
}

/// Print every registered model's id and human name.
fn print_models(registry: &[ModelEntry]) {
    println!("available models:");
    for entry in registry {
        let (id, name) = (&entry.id, &entry.name);
        println!("  {id:<18} {name}");
    }
}

/// Returns one model's parameter descriptors: the ids `--set` accepts, with kinds, defaults and
/// bounds.
///
/// Emitted as `key=value` fields rather than a formatted table so `scripts/bench_matrix.py` can
/// read a model's axes (does it have `grid_width`? `num_agents`? at what default?) instead of
/// hard-coding per-model knowledge or probing with throwaway runs.
fn params_text(entry: &ModelEntry) -> String {
    let mut text = format!("parameters for {} ({}):\n", entry.id, entry.name);
    for (index, desc) in entry.param_descriptors.iter().enumerate() {
        let (id, label) = (desc.id, desc.label);
        let apply = if desc.is_live() { "live" } else { "reload" };
        let kind = match &desc.kind {
            ParamKind::F32 { min, max, default, .. } => format!("kind=f32 default={default} min={min} max={max}"),
            ParamKind::U32 { min, max, default } => format!("kind=u32 default={default} min={min} max={max}"),
            ParamKind::Bool { default } => format!("kind=bool default={default}"),
            ParamKind::Choice { options, default } => {
                format!("kind=choice default={default} options={}", options.join("|"))
            }
        };
        // Values are always fractions, whatever the panel shows, so this only describes how to read one.
        let format = match desc.format {
            ParamFormat::Plain => "",
            ParamFormat::Percent => " format=percent",
        };
        text.push_str(&format!(
            "  index={index} id={id} {kind} apply={apply}{format} label=\"{label}\"\n"
        ));
    }
    text
}

/// Create a fresh CPU state from a registry entry. Errors on a GPU-backed model, for which
/// callers use [`new_gpu_state`]. The dispatcher in [`run_benchmark`] routes correctly, so this
/// only fires for a CPU-only path handed a GPU model, such as `--export`.
fn new_cpu_state(entry: &ModelEntry, params: &[ParamValue], seed: Option<u64>) -> Result<Box<dyn SimState>> {
    match (entry.create)(params, seed)? {
        ModelState::Cpu(state) => Ok(state),
        ModelState::Gpu(_) => bail!("model '{}' is GPU-backed; this path is CPU-only", entry.id),
    }
}

/// Dispatch to the CPU or GPU benchmark depending on which backend the registry entry produces.
/// The probe state created here is thrown away, and each per-rep loop builds its own.
fn run_benchmark(
    entry: &ModelEntry,
    params: &[ParamValue],
    args: &Args,
    schedule: &Schedule,
    gpu_ctx: Option<&GpuContext>,
    adapter: Option<&str>,
) -> Result<()> {
    let (is_gpu, jobs) = match (entry.create)(params, args.seed)? {
        ModelState::Gpu(_) => (true, None),
        ModelState::Cpu(state) => (false, state.parallel_jobs()),
    };
    if is_gpu {
        let ctx = gpu_ctx.context("GPU model selected but no GPU device is available")?;
        if args.json {
            json_report::info(&entry.id, "gpu", rayon::current_num_threads(), jobs, adapter);
        }
        bench_gpu(entry, params, args, schedule, ctx)
    } else {
        if args.json {
            json_report::info(&entry.id, "cpu", rayon::current_num_threads(), jobs, None);
        }
        bench_cpu(entry, params, args, schedule)
    }
}

/// Rep `i`'s seed, which `benchmarks/protocol.md` fixes as `base + i`.
///
/// Without it every rep replays one trajectory and the spread across reps is timer jitter.
fn rep_seed(base: Option<u64>, rep: u64) -> Option<u64> {
    base.map(|s| s.wrapping_add(rep))
}

/// CPU benchmark: for each rep, build a fresh state, warm it up untimed, then time `steps` steps.
/// Timing wraps *only* the step loop, so state construction and warmup allocation stay out of the
/// measured window.
fn bench_cpu(entry: &ModelEntry, params: &[ParamValue], args: &Args, schedule: &Schedule) -> Result<()> {
    eprintln!(
        "benchmarking {} ({}): {} steps x {} reps, {} warmup, {} global-warmup",
        entry.name, entry.id, args.steps, args.reps, args.warmup, args.global_warmup
    );
    if cfg!(debug_assertions) {
        eprintln!("!!! warning: debug build; use --release for benchmarking !!!");
    }

    // Rep 0: optional one-time warm-up (CPU turbo / caches) before the timed reps. See
    // `--global-warmup`; a no-op at 0.
    if args.global_warmup > 0 {
        let mut warm = new_cpu_state(entry, params, args.seed)?;
        eprint!("  #{: >4}: ", 0);
        let start = Instant::now();
        rayon::scope(|_| {
            for _ in 0..args.global_warmup {
                warm.step();
            }
        });
        let elapsed = start.elapsed();
        eprintln!("{elapsed:>8.3?}  ({0} global warmup steps)", args.global_warmup);
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(args.reps as usize);
    let mut population: u64 = 0;
    // Grid dimensions read from the running state, not the params, since the state is
    // authoritative. It reflects any `--set grid_width=…` override and what the model built.
    let mut grid_dims: Option<(u32, u32)> = None;

    for rep in 0..args.reps {
        let seed = rep_seed(args.seed, rep);
        let mut state = new_cpu_state(entry, params, seed)?;
        // Stepped from inside the pool. This thread is not a rayon worker, so every parallel pass
        // a kernel runs would otherwise be injected from outside and park the caller until it
        // finishes. One inject per rep replaces one per pass per step.
        rayon::scope(|_| {
            for _ in 0..args.warmup {
                note_refused(schedule.run_due(&mut *state));
                state.step();
            }
        });
        // For grid models `population()` is the total cell count (width×height); for agent models
        // it is the agent count. Either way it is the right denominator for agent-updates/sec.
        // A network model's population can change as it runs, so it is read again after the timed steps.
        population = state.population();
        let heap_bytes = state.heap_bytes();
        grid_dims = state.grid_view().map(|g| (g.width, g.height));

        eprint!("  #{: >4}: ", rep + 1);
        let start = Instant::now();
        rayon::scope(|_| {
            for _ in 0..args.steps {
                note_refused(schedule.run_due(&mut *state));
                state.step();
            }
        });
        let elapsed = start.elapsed();
        // Outside the timer, so an action on the last tick still lands without being measured.
        note_refused(schedule.run_due(&mut *state));
        eprintln!("{elapsed:>8.3?}");
        // A population that fluctuates at steady state moves by about its square root, which is not worth a note.
        let after = state.population();
        let noise = (3.0 * (population as f64).sqrt()) as u64;
        if after.abs_diff(population) > (population / 10).max(noise) {
            eprintln!(
                "  note: the population went from {population} to {after} during the timed steps. \
                 Updates per second are computed from the population after warmup. \
                 A longer --warmup can reach a steady population first."
            );
        }
        samples.push(elapsed);
        if args.json {
            json_report::rep(
                rep,
                seed,
                args.steps,
                args.warmup,
                elapsed,
                population,
                Some(heap_bytes),
            );
        }
    }

    if args.json {
        json_report::summary(
            &samples,
            args.steps,
            population,
            grid_dims,
            &entry.param_descriptors,
            params,
            schedule,
        );
    } else {
        report(&samples, args.steps, population, grid_dims)?;
    }
    Ok(())
}

/// Median of an already sorted slice, averaging the middle two on an even count.
///
/// The driver's `statistics.median` does the same, and a lone middle sample disagreed with it.
pub fn median_of(sorted: &[Duration]) -> Duration {
    match sorted.len() {
        0 => Duration::ZERO,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2,
    }
}

/// Turn the raw per-rep timings into the reported benchmark result.
///
/// - `samples`: one wall-clock [`Duration`] per rep, each covering `steps_per_rep` steps. Never
///   empty (`--reps` is at least 1).
/// - `steps_per_rep`: how many `step()` calls each sample covers.
/// - `population`: agent count sampled after warmup (see [`run_benchmark`]).
/// - `grid_dims`: `(width, height)` for grid models, read from the live state; `None` otherwise.
fn report(samples: &[Duration], steps_per_rep: u64, population: u64, grid_dims: Option<(u32, u32)>) -> Result<()> {
    println!("benchmark result:");
    // `samples` is non-empty (`--reps` >= 1), so the defaults are never actually used.
    let min = samples.iter().min().copied().unwrap_or_default();
    let max = samples.iter().max().copied().unwrap_or_default();
    let mean = samples.iter().sum::<Duration>() / (samples.len() as u32);
    let median = {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        median_of(&sorted)
    };
    let std_dev = {
        let mean_secs = mean.as_secs_f64();
        let variance = samples
            .iter()
            .map(|s| {
                let diff = s.as_secs_f64() - mean_secs;
                diff * diff
            })
            .sum::<f64>()
            / (samples.len() as f64);
        Duration::from_secs_f64(variance.sqrt())
    };

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
    let mean_updates_per_sec = f.fmt2((steps_per_rep as f64 * population as f64) / mean.as_secs_f64());
    println!("  > mean updates/sec: {mean_updates_per_sec:>20}");
    if let Some((w, h)) = grid_dims {
        f = f.precision(numfmt::Precision::Decimals(0));
        let grid_size = f.fmt2(w as u64 * h as u64);
        println!("  > grid size:        {grid_size:>16}");
    }
    Ok(())
}

/// GPU benchmark. GPU state never leaves the device, and `SimState::step()` submits one tiny
/// command buffer per step without ever waiting. Each rep instead batches `steps` into GPU
/// submissions and blocks on completion. See [`run_gpu_rep`].
fn bench_gpu(
    entry: &ModelEntry,
    params: &[ParamValue],
    args: &Args,
    schedule: &Schedule,
    ctx: &GpuContext,
) -> Result<()> {
    eprintln!(
        "benchmarking {} ({}) [GPU]: {} steps x {} reps, {} warmup, {} global-warmup",
        entry.name, entry.id, args.steps, args.reps, args.warmup, args.global_warmup
    );
    if cfg!(debug_assertions) {
        eprintln!("!!! warning: debug build; use --release for benchmarking !!!");
    }

    // Optional one-time hardware warm-up. Steps a throwaway state to ramp the GPU off its idle
    // clocks (DVFS) and pay first-use shader compilation before any timed rep, so rep 1 isn't
    // cold. Off by default, since its cost scales with the workload.
    if args.global_warmup > 0 {
        let mut warm = new_gpu_state(entry, params, args.seed)?;
        eprint!("  #{: >4}: ", 0);
        let start = Instant::now();
        stepping::run_steps(&mut *warm, ctx, args.global_warmup)?;
        let elapsed = start.elapsed();
        eprintln!("{elapsed:>8.3?}  ({0} global warmup steps)", args.global_warmup);
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(args.reps as usize);
    let mut population: u64 = 0;

    for rep in 0..args.reps {
        let seed = rep_seed(args.seed, rep);
        let mut state = new_gpu_state(entry, params, seed)?;
        let (after_warmup, elapsed) = run_gpu_rep(&mut *state, ctx, args.warmup, args.steps, schedule)?;
        population = after_warmup;
        eprintln!("  #{: >4}: {elapsed:>8.3?}", rep + 1);
        samples.push(elapsed);
        if args.json {
            json_report::rep(rep, seed, args.steps, args.warmup, elapsed, population, None);
        }
    }

    // GPU state exposes no `grid_view()`, so derive dimensions from the resolved params, which
    // are exactly what the model was built from.
    let grid_dims = grid_dims_from_params(&entry.param_descriptors, params);
    if args.json {
        json_report::summary(
            &samples,
            args.steps,
            population,
            grid_dims,
            &entry.param_descriptors,
            params,
            schedule,
        );
    } else {
        report(&samples, args.steps, population, grid_dims)?;
    }
    Ok(())
}

/// Create a fresh GPU state from a registry entry. Errors on a CPU-backed model.
fn new_gpu_state(entry: &ModelEntry, params: &[ParamValue], seed: Option<u64>) -> Result<Box<dyn GpuSimState>> {
    match (entry.create)(params, seed)? {
        ModelState::Gpu(state) => Ok(state),
        ModelState::Cpu(_) => bail!("expected a GPU model but '{}' is CPU-backed", entry.id),
    }
}

/// Runs one GPU benchmark rep on `state` and returns its population after warm-up and the time its timed steps took.
///
/// The rep runs `warmup` untimed steps and then `steps` timed ones, both under [`BENCH_FIRE`]. An action due from the
/// end of warm-up up to, but not including, the tick the rep stops on is timed with the steps. One due on the tick the
/// rep stops on fires after the timer stops. It is waited for there. Otherwise its work would land in the next rep,
/// inside the timer when that rep has no warm-up, and a fault it raised would be reported late or not at all.
fn run_gpu_rep(
    state: &mut dyn GpuSimState,
    ctx: &GpuContext,
    warmup: u64,
    steps: u64,
    schedule: &Schedule,
) -> Result<(u64, Duration)> {
    note_refused(stepping::run_steps_acting(state, ctx, warmup, schedule, BENCH_FIRE)?);
    let population = state.population();

    let start = Instant::now();
    let refused = stepping::run_steps_acting(state, ctx, steps, schedule, BENCH_FIRE)?;
    let elapsed = start.elapsed();
    note_refused(refused);
    note_refused(stepping::run_due(state, ctx, schedule));
    stepping::wait(ctx)?;
    Ok((population, elapsed))
}

/// Best-effort grid dimensions from the resolved params, for GPU models whose state exposes no
/// `grid_view`. Reads the engine-injected `grid_width` / `grid_height` u32 params.
fn grid_dims_from_params(descriptors: &[ParamDescriptor], params: &[ParamValue]) -> Option<(u32, u32)> {
    let find = |id: &str| -> Option<u32> {
        let index = descriptors.iter().position(|d| d.id == id)?;
        match params.get(index) {
            Some(ParamValue::U32(v)) => Some(*v),
            _ => None,
        }
    };
    Some((find("grid_width")?, find("grid_height")?))
}

/// Run once (warmup + steps) and write the final state to `path`.
fn export_final(
    entry: &ModelEntry,
    params: &[ParamValue],
    args: &Args,
    schedule: &Schedule,
    path: &Path,
) -> Result<()> {
    let mut state = new_cpu_state(entry, params, args.seed)?;
    for _ in 0..(args.warmup + args.steps) {
        note_refused(schedule.run_due(&mut *state));
        state.step();
    }
    note_refused(schedule.run_due(&mut *state));
    write_state(&mut *state, path)?;
    eprintln!("exported final state (tick {}) to {}", state.tick(), path.display());
    Ok(())
}

/// Run one configuration and write the per-tick stat series to `path` as CSV.
///
/// Unlike [`run_benchmark`] this is not a timed path: sampling stats every tick is itself
/// significant work (a full reduction over the grid for a `GridModel`), so numbers from a run with
/// `--export-stats` are not comparable to a benchmark run. Reps are ignored, since a time series
/// is one trajectory and N reps would be N different trajectories in one file.
fn export_stats(
    entry: &ModelEntry,
    params: &[ParamValue],
    args: &Args,
    schedule: &Schedule,
    path: &Path,
    gpu_ctx: Option<&GpuContext>,
) -> Result<()> {
    if args.stats_every == 0 {
        bail!("--stats-every must be at least 1");
    }
    if args.reps > 1 {
        eprintln!("note: --reps is ignored by --export-stats");
    }

    let total = args.warmup + args.steps;
    let file = File::create(path).with_context(|| format!("cannot create '{}'", path.display()))?;
    let writer = StatsWriter::new(BufWriter::new(file));

    eprintln!(
        "exporting stats for {} ({}): {} steps, sampling every {}",
        entry.name, entry.id, total, args.stats_every
    );

    let rows = match (entry.create)(params, args.seed)? {
        ModelState::Cpu(state) => stats_cpu(state, args, schedule, total, writer)?,
        ModelState::Gpu(state) => {
            let ctx = gpu_ctx.context("GPU model selected but no GPU device is available")?;
            stats_gpu(state, ctx, args, schedule, total, writer)?
        }
    };

    eprintln!("wrote {rows} rows to {}", path.display());
    Ok(())
}

/// CPU stat sampling: step and sample in one loop. Tick 0 (the initial state, before any step) is
/// recorded so the series starts from the model's initial conditions.
fn stats_cpu<W: std::io::Write>(
    mut state: Box<dyn SimState>,
    args: &Args,
    schedule: &Schedule,
    total: u64,
    mut writer: StatsWriter<W>,
) -> Result<u64> {
    // A sample is taken as a publish would take it. A stat that walks the graph is computed in `prepare_view`.
    let sample = |state: &mut dyn SimState, writer: &mut StatsWriter<W>| -> Result<()> {
        state.prepare_view();
        writer.push(state.tick(), &state.stats())?;
        Ok(())
    };
    note_refused(schedule.run_due(&mut *state));
    sample(&mut *state, &mut writer)?;
    for i in 0..total {
        state.step();
        note_refused(schedule.run_due(&mut *state));
        if (i + 1).is_multiple_of(args.stats_every) {
            sample(&mut *state, &mut writer)?;
        }
    }
    // The final tick always lands in the file even off a sampling boundary. The end state of a
    // run is the one value a reader is most likely to want.
    if !total.is_multiple_of(args.stats_every) {
        sample(&mut *state, &mut writer)?;
    }
    Ok(writer.finish()?)
}

/// GPU stat sampling. `SimState::stats()` on a GPU state returns whatever the last completed
/// readback produced, so each sample needs the full encode-reduce-readback round trip and a
/// blocking poll, or every row would repeat a stale value. That makes the sampling interval the
/// dominant cost here, and `--stats-every` is how you buy it back.
fn stats_gpu<W: std::io::Write>(
    mut state: Box<dyn GpuSimState>,
    ctx: &GpuContext,
    args: &Args,
    schedule: &Schedule,
    total: u64,
    mut writer: StatsWriter<W>,
) -> Result<u64> {
    let sample = |state: &mut dyn GpuSimState, writer: &mut StatsWriter<W>| -> Result<()> {
        let stats = stepping::sample_stats(state, ctx);
        writer.push(state.tick(), &stats)?;
        Ok(())
    };

    note_refused(stepping::run_due(&mut *state, ctx, schedule));
    sample(&mut *state, &mut writer)?;
    let mut done = 0;
    while done < total {
        let chunk = args.stats_every.min(total - done);
        let refused = stepping::run_steps_acting(&mut *state, ctx, chunk, schedule, Fire::AfterStep)?;
        note_refused(refused);
        done += chunk;
        sample(&mut *state, &mut writer)?;
    }
    // A fault raised by the last sample is reported here. No later wait would report it.
    stepping::wait(ctx)?;
    Ok(writer.finish()?)
}

/// Serializes a CPU model's view to a simple text format.
///
/// A grid is written as comma-separated cell indices per row, a point cloud as `x,y` CSV with a `color` column if
/// the model carries the lane, and network edges as `src,dst,color` rows that index into the points.
///
/// Every section the model has is written, so a composite model exports both its field and its agents.
fn write_state(state: &mut dyn SimState, path: &Path) -> Result<()> {
    // The display layer is only refreshed on publish, and an export is a publish.
    state.prepare_view();
    let grid = state.grid_view();
    let points = state.point_view();
    if grid.is_none() && points.is_none() {
        bail!("model exposes no CPU-side view to export");
    }

    let file = File::create(path).with_context(|| format!("cannot create '{}'", path.display()))?;
    let mut out = BufWriter::new(file);

    if let Some(grid) = grid {
        state_export::write_grid(&mut out, grid.width, grid.height, grid.cells)?;
    }

    if let Some(points) = &points {
        state_export::write_points(&mut out, points.pos_x, points.pos_y, points.color)?;
    }

    if let (Some(points), Some(edges)) = (&points, state.edge_view()) {
        let rows = state_export::point_rows(points.pos_x, points.pos_y);
        state_export::write_edges(&mut out, edges.src, edges.dst, edges.color.unwrap_or(&[]), &rows)?;
    }

    Ok(())
}

/// Converts an error from resolving `--set` into the one the command line reports, naming the flag.
fn set_error(error: ValueError) -> anyhow::Error {
    match error {
        ValueError::Param { id, source } => anyhow::Error::new(*source).context(format!("--set {id}")),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;

    use super::{Args, Mode, params_text, rep_seed, run_gpu_rep, stats_cpu, stats_gpu};
    use crate::explore::{self, ExploreArgs};
    use crate::json_report;
    use clap::Parser as _;
    use henad_compute::gpu::{GpuContext, GpuSimState};
    use henad_core::action::Schedule;
    use henad_core::explore::seed::run_seed;
    use henad_core::explore::value::{parse_overrides, resolve_params};
    use henad_core::export::csv::parse_records;
    use henad_core::export::stats_csv::StatsWriter;
    use henad_core::params::ParamValue;
    use henad_explore::device::acquire_headless;
    use henad_models::registry::{ModelEntry, ModelState, model_registry};
    use serde_json::json;

    /// Directory under the system's temporary directory, unique to one test, removed with its contents on drop.
    struct ScratchDir {
        path: PathBuf,
    }

    impl ScratchDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("henad-cli-{name}-{}", std::process::id()));
            if path.exists() {
                std::fs::remove_dir_all(&path).expect("an earlier run's directory can be removed");
            }
            Self { path }
        }

        fn arg(&self) -> &str {
            self.path.to_str().expect("the temporary directory is UTF-8")
        }

        fn read(&self, file: &str) -> String {
            std::fs::read_to_string(self.path.join(file)).expect("the sweep wrote the file")
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.path).ok();
        }
    }

    fn cpu_entry(id: &str) -> ModelEntry {
        model_registry(None)
            .into_iter()
            .find(|entry| entry.id == id)
            .expect("the model is registered")
    }

    /// Returns the lines of a sweep's `series.csv`, header included, without the `run_id` column.
    fn series_without_run_id(dir: &ScratchDir) -> Vec<String> {
        dir.read("series.csv")
            .lines()
            .map(|line| line.split_once(',').expect("a run_id column").1.to_owned())
            .collect()
    }

    /// Checks that every `:final` reducer of the one run in `dir` equals the value of its column in `last_row`.
    ///
    /// `header` names the columns of `last_row`, as a stats export writes them.
    fn assert_final_reducers(dir: &ScratchDir, header: &str, last_row: &str) {
        let runs = parse_records(&dir.read("runs.csv")).expect("runs.csv is CSV");
        assert_eq!(runs.len(), 2, "a header and one run");
        for (column, value) in header.split(',').zip(last_row.split(',')).skip(1) {
            let final_column = runs[0]
                .iter()
                .position(|name| *name == format!("{column}:final"))
                .expect("every stat column has a final reducer");
            let reduced: f64 = runs[1][final_column].parse().expect("a number");
            let exported: f64 = value.parse().expect("a number");
            assert_eq!(reduced, exported, "{column}");
        }
    }

    /// Checks that a sweep of one run writes the series `--export-stats` writes for the run's seed.
    ///
    /// 31 steps sampled every 3 ticks end off the sampling boundary, so the final tick is a row of its own.
    #[test]
    fn a_single_point_sweep_matches_export_stats() {
        let entry = cpu_entry("sir");
        let dir = ScratchDir::new("single-point");
        let args = Args::parse_from([
            "henad-cli",
            "sir",
            "--set",
            "grid_width=32",
            "--set",
            "grid_height=32",
            "--steps",
            "31",
            "--stats-every",
            "3",
            "--seed",
            "7",
            "--out",
            dir.arg(),
        ]);
        let status = explore::run(&args, &entry, None, None, None).expect("the sweep runs");
        assert_eq!(status, ExitCode::SUCCESS);

        let overrides = parse_overrides(&args.set).expect("valid");
        let params = resolve_params(&entry.param_descriptors, &overrides).expect("in range");
        let Ok(ModelState::Cpu(state)) = (entry.create)(&params, Some(run_seed(7, 0))) else {
            panic!("sir builds as a CPU model");
        };
        let schedule = Schedule::parse(&[], &entry.id, &entry.action_descriptors).expect("no actions");
        let mut exported_bytes = Vec::new();
        stats_cpu(state, &args, &schedule, 31, StatsWriter::new(&mut exported_bytes)).expect("writes");
        let exported = String::from_utf8(exported_bytes).expect("utf-8");
        let exported: Vec<&str> = exported.lines().collect();

        assert_eq!(series_without_run_id(&dir), exported);
        assert_eq!(exported.len(), 1 + 12, "a header, ticks 0 to 30 every 3, and tick 31");
        assert_final_reducers(&dir, exported[0], exported[exported.len() - 1]);
    }

    /// Checks that a sweep fires each `--act` as `--export-stats` fires it, the tick the run ends on included.
    #[test]
    fn a_sweep_fires_its_actions_as_export_stats_does() {
        let entry = cpu_entry("sir");
        let dir = ScratchDir::new("single-point-actions");
        let args = Args::parse_from([
            "henad-cli",
            "sir",
            "--set",
            "grid_width=32",
            "--set",
            "grid_height=32",
            "--steps",
            "20",
            "--stats-every",
            "2",
            "--seed",
            "7",
            "--act",
            "seed_outbreak@0",
            "--act",
            "seed_outbreak@5",
            "--act",
            "seed_outbreak@20",
            "--out",
            dir.arg(),
        ]);
        let status = explore::run(&args, &entry, None, None, None).expect("the sweep runs");
        assert_eq!(status, ExitCode::SUCCESS);

        let overrides = parse_overrides(&args.set).expect("valid");
        let params = resolve_params(&entry.param_descriptors, &overrides).expect("in range");
        let Ok(ModelState::Cpu(state)) = (entry.create)(&params, Some(run_seed(7, 0))) else {
            panic!("sir builds as a CPU model");
        };
        let schedule = Schedule::parse(&args.act, &entry.id, &entry.action_descriptors).expect("declared actions");
        let mut exported_bytes = Vec::new();
        stats_cpu(state, &args, &schedule, 20, StatsWriter::new(&mut exported_bytes)).expect("writes");
        let exported = String::from_utf8(exported_bytes).expect("utf-8");
        assert_eq!(series_without_run_id(&dir), exported.lines().collect::<Vec<_>>());
    }

    /// Returns the records of `runs.csv` in `dir`, header included, without the columns that time a run.
    fn runs_without_timing(dir: &Path) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(dir.join("runs.csv")).expect("the sweep wrote runs.csv");
        let mut records = parse_records(&text).expect("runs.csv is CSV");
        let timing: Vec<usize> = records[0]
            .iter()
            .enumerate()
            .filter(|(_, name)| matches!(name.as_str(), "build_ms" | "wall_ms" | "steps_per_s"))
            .map(|(column, _)| column)
            .collect();
        assert_eq!(timing.len(), 3, "runs.csv times each run in three columns");
        for record in &mut records {
            for &column in timing.iter().rev() {
                record.remove(column);
            }
        }
        records
    }

    /// Checks that two shards of a sampled sweep, merged, hold the files of the sweep run in one go.
    ///
    /// The sweep samples a parameter and an action's tick, and stops a run once nobody is infected. Some runs stop
    /// before their last tick, so their series end early.
    #[test]
    fn sharded_sweeps_merge_into_the_unsharded_files() {
        let entry = cpu_entry("sir");
        let dir = ScratchDir::new("shards");
        std::fs::create_dir_all(&dir.path).expect("the scratch directory can be made");
        let sweep = |dest: &Path, shard: Option<&str>| {
            let dest = dest.to_str().expect("the temporary directory is UTF-8");
            let mut line = vec![
                "henad-cli",
                "sir",
                "--set",
                "grid_width=16",
                "--set",
                "grid_height=16",
                "--set",
                "initial_infected_pct=0.02",
                "--set",
                "recovery_rate=0.3",
                "--steps",
                "24",
                "--stats-every",
                "3",
                "--series-every",
                "6",
                "--reps",
                "2",
                "--seed",
                "11",
                "--vary",
                "infection_rate=0.05:0.6",
                "--act",
                "seed_outbreak@6",
                "--vary",
                "action.seed_outbreak=2:20",
                "--sample",
                "lhs:3",
                "--stop",
                "Infected <= 0",
                "--reduce",
                "Infected:argmax",
                "--reduce",
                "Infected:first<=1",
                "--out",
                dest,
            ];
            line.extend(shard.map(|shard| ["--shard", shard]).into_iter().flatten());
            let args = Args::parse_from(line);
            explore::run(&args, &entry, None, None, None).expect("the sweep runs")
        };
        let whole = dir.path.join("whole");
        assert_eq!(sweep(&whole, None), ExitCode::SUCCESS);
        let shards = [dir.path.join("shard-0"), dir.path.join("shard-1")];
        for (shard_dir, shard) in shards.iter().zip(["0/2", "1/2"]) {
            assert_eq!(sweep(shard_dir, Some(shard)), ExitCode::SUCCESS, "shard {shard}");
            assert_eq!(
                runs_without_timing(shard_dir).len(),
                1 + 3,
                "a header and every other run of 6"
            );
        }

        let path = |dir: &Path| dir.to_str().expect("the temporary directory is UTF-8").to_owned();
        let merged = dir.path.join("merged");
        let args = Args::parse_from([
            "henad-cli".to_owned(),
            "--merge".to_owned(),
            path(&shards[1]),
            path(&shards[0]),
            "--out".to_owned(),
            path(&merged),
        ]);
        assert_eq!(Mode::of(&args), Mode::Merge);
        assert_eq!(
            explore::merge_shards(&args).expect("the shards merge"),
            ExitCode::SUCCESS
        );
        assert_eq!(runs_without_timing(&merged), runs_without_timing(&whole));
        for file in ["series.csv", "summary.csv"] {
            let read = |dir: &Path| std::fs::read_to_string(dir.join(file)).expect("the table is written");
            assert_eq!(read(&merged), read(&whole), "{file}");
        }

        let partial = dir.path.join("partial");
        let args = Args::parse_from([
            "henad-cli".to_owned(),
            "--merge".to_owned(),
            path(&shards[0]),
            "--out".to_owned(),
            path(&partial),
        ]);
        let status = explore::merge_shards(&args).expect("one shard merges");
        assert_eq!(
            status,
            ExitCode::from(explore::SOME_RUNS_NOT_OK),
            "half the runs are missing"
        );
    }

    /// Checks that `--params` prints what it printed before sweeps, byte for byte.
    #[test]
    fn params_text_output_is_unchanged() {
        let expected = "\
parameters for virus_network (Virus on a Network):
  index=0 id=num_agents kind=u32 default=10000 min=1 max=10000000 apply=reload label=\"Number of Nodes\"
  index=1 id=world_width kind=f32 default=1000 min=1 max=10000 apply=reload label=\"World Width\"
  index=2 id=world_height kind=f32 default=1000 min=1 max=10000 apply=reload label=\"World Height\"
  index=3 id=average_node_degree kind=u32 default=6 min=1 max=20 apply=reload label=\"Average Node Degree\"
  index=4 id=initial_outbreak_size kind=u32 default=3 min=1 max=10000 apply=reload label=\"Initial Outbreak Size\"
  index=5 id=virus_spread_chance kind=f32 default=0.025 min=0 max=1 apply=live format=percent label=\"Virus Spread Chance\"
  index=6 id=virus_check_frequency kind=u32 default=1 min=1 max=20 apply=live label=\"Virus Check Frequency\"
  index=7 id=recovery_chance kind=f32 default=0.05 min=0 max=1 apply=live format=percent label=\"Recovery Chance\"
  index=8 id=gain_resistance_chance kind=f32 default=0.05 min=0 max=1 apply=live format=percent label=\"Gain Resistance Chance\"
  index=9 id=directed kind=bool default=false apply=live label=\"Directed\"
  index=10 id=network kind=choice default=0 options=Random|Geometric apply=reload label=\"Network\"
  index=11 id=keep_rewiring kind=bool default=false apply=live label=\"Keep Rewiring\"
";
        assert_eq!(params_text(&cpu_entry("virus_network")), expected);
    }

    #[test]
    fn params_json_lists_every_descriptor() {
        for entry in model_registry(None) {
            let line = json_report::params(&entry, None);
            assert_eq!(line["kind"], json!("params"), "{}", entry.id);
            assert_eq!(line["model"], json!(entry.id));
            let params = line["params"].as_array().expect("params is a list");
            let ids: Vec<&str> = params.iter().filter_map(|param| param["id"].as_str()).collect();
            let declared: Vec<&str> = entry.param_descriptors.iter().map(|d| d.id).collect();
            assert_eq!(ids, declared, "{}", entry.id);
            for param in params {
                assert!(param["kind"].is_string() && !param["default"].is_null(), "{param}");
            }
            let columns = line["stat_columns"]
                .as_array()
                .expect("the model builds at its defaults");
            assert!(columns.len() >= entry.stat_descriptors.len(), "{}", entry.id);
            let actions = line["actions"].as_array().expect("actions is a list");
            assert_eq!(actions.len(), entry.action_descriptors.len(), "{}", entry.id);
        }
    }

    /// Returns the mode a command line had before sweeps existed.
    fn legacy_mode(args: &Args) -> Mode {
        if args.list {
            Mode::List
        } else if args.model.is_none() {
            Mode::InfoOnly
        } else if args.params {
            Mode::Params
        } else if args.export_stats.is_some() {
            Mode::ExportStats
        } else if args.export.is_some() {
            Mode::ExportFinal
        } else {
            Mode::Benchmark
        }
    }

    /// Returns every `henad-cli` command line in `text`, without the program name.
    ///
    /// A line is one that starts `henad-cli` or `cargo run --release -p henad-cli --`, after any `//!`, with a
    /// trailing `\` joining it to the next. The usage line, `henad-cli [OPTIONS] [MODEL]`, is left out.
    fn command_lines(text: &str) -> Vec<Vec<String>> {
        let mut lines = Vec::new();
        let mut pending = String::new();
        for raw in text.lines() {
            let raw = raw.trim_start().trim_start_matches("//!").trim();
            if let Some(start) = raw.strip_suffix('\\') {
                pending.push_str(start);
                continue;
            }
            pending.push_str(raw);
            let line = std::mem::take(&mut pending);
            let rest = line
                .strip_prefix("cargo run --release -p henad-cli -- ")
                .or_else(|| line.strip_prefix("henad-cli "));
            if let Some(rest) = rest.filter(|rest| !rest.starts_with('[')) {
                lines.push(rest.split_whitespace().map(str::to_owned).collect());
            }
        }
        lines
    }

    /// Checks that every command line the docs and scripts pass keeps the mode it had before sweeps.
    ///
    /// The script lines are written the way `scripts/compare_bench.py`, `bench_matrix.py`, `compare_sir.py` and
    /// `compare_network.py` build them. A documented sweep line enters explore mode, and a merge line merge mode.
    #[test]
    fn existing_invocations_keep_their_mode() {
        // Read at run time. The page sits outside the package, and a crate built from its tarball skips the test.
        let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/cli.md");
        let Ok(reference) = std::fs::read_to_string(&reference) else {
            eprintln!("note: skipped, {} is absent", reference.display());
            return;
        };
        let mut lines = command_lines(&reference);
        lines.extend(command_lines(include_str!("main.rs")));
        let documented = lines.len();
        let scripts = [
            "boids --json --steps 100 --warmup 10 --reps 5 --seed 42 --threads 1 --set num_agents=10000",
            "sir --json --steps 100 --warmup 10 --reps 5 --seed 42 --threads 0 --set grid_width=256 \
             --set grid_height=256",
            "gpu_boids --json --steps 100 --warmup 10 --reps 5 --seed 42 --threads 0 --global-warmup 1000 \
             --set num_agents=10000",
            "sir --params",
            "--list",
            "--info --json",
            "sir --set grid_width=100 --set grid_height=100 --set infection_rate=0.3 --set recovery_rate=0.05 \
             --set initial_infected_pct=0.01 --steps 200 --seed 1 --export-stats sir_henad_001.csv",
            "virus_network --set num_agents=150 --steps 200 --seed 1 --export-stats virus_henad_001.csv",
        ];
        lines.extend(
            scripts
                .iter()
                .map(|line| line.split_whitespace().map(str::to_owned).collect()),
        );

        let mut sweeps = 0;
        for line in &lines {
            let argv = std::iter::once("henad-cli").chain(line.iter().map(String::as_str));
            let args = Args::try_parse_from(argv).unwrap_or_else(|error| panic!("{line:?} parses: {error}"));
            if args.explore == ExploreArgs::default() {
                assert_eq!(Mode::of(&args), legacy_mode(&args), "{line:?}");
            } else {
                let mode = if args.explore.merge.is_empty() {
                    Mode::Explore
                } else {
                    Mode::Merge
                };
                assert_eq!(Mode::of(&args), mode, "{line:?}");
                sweeps += 1;
            }
        }
        assert!(
            documented - sweeps >= 12,
            "found {documented} documented lines, {sweeps} of them sweeps"
        );
        assert_eq!(rep_seed(Some(40), 2), Some(42), "a benchmark rep's seed stays base + i");
    }

    /// Each exported sample is prepared as a publish would be, so a stat computed in `prepare_view` is current.
    /// Team Assembly's component stats are computed there.
    ///
    /// With `p` at zero every tick adds a separate clique of four newcomers, and nothing retires yet.
    /// Rows land at tick 0, at tick 2 on the sampling boundary, and at the final tick 3 off it.
    #[test]
    fn exported_stats_are_prepared_like_a_publish() {
        let entry = model_registry(None)
            .into_iter()
            .find(|e| e.id == "team_assembly")
            .expect("team_assembly is registered");
        let overrides =
            parse_overrides(&["num_agents=4".to_owned(), "team_size=4".to_owned(), "p=0".to_owned()]).expect("valid");
        let params = resolve_params(&entry.param_descriptors, &overrides).expect("in range");
        let args = Args::parse_from(["henad-cli", "team_assembly", "--stats-every", "2"]);
        let schedule = Schedule::parse(&[], &entry.id, &entry.action_descriptors).expect("no actions");
        let Ok(ModelState::Cpu(state)) = (entry.create)(&params, Some(1)) else {
            panic!("team_assembly builds as a CPU model");
        };

        let mut out = Vec::new();
        let rows = stats_cpu(state, &args, &schedule, 3, StatsWriter::new(&mut out)).expect("writes");
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

    /// Returns whether `HENAD_REQUIRE_GPU` turns a missing device into a failure. Empty and `0` read as unset.
    fn gpu_required() -> bool {
        std::env::var_os("HENAD_REQUIRE_GPU").is_some_and(|v| !v.is_empty() && v != "0")
    }

    /// A 64 by 64 GPU grid model and the device it runs on.
    struct SmallGpuGrid {
        ctx: GpuContext,
        entry: ModelEntry,
        params: Vec<ParamValue>,
    }

    impl SmallGpuGrid {
        /// Returns model `id` on a fresh device, or `None` to skip the test when this machine has no device.
        ///
        /// # Panics
        ///
        /// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
        fn new(id: &str) -> Option<Self> {
            let ctx = match acquire_headless() {
                Ok((ctx, _)) => ctx,
                Err(err) => {
                    assert!(
                        !gpu_required(),
                        "HENAD_REQUIRE_GPU is set but no device is available: {err:?}"
                    );
                    return None;
                }
            };
            let entry = model_registry(Some(ctx.clone()))
                .into_iter()
                .find(|e| e.id == id)
                .expect("the model is registered");
            let overrides = parse_overrides(&["grid_width=64".to_owned(), "grid_height=64".to_owned()]).expect("valid");
            let params = resolve_params(&entry.param_descriptors, &overrides).expect("in range");
            Some(Self { ctx, entry, params })
        }

        fn schedule(&self, raw: &[&str]) -> Schedule {
            let raw: Vec<String> = raw.iter().map(|&s| s.to_owned()).collect();
            Schedule::parse(&raw, &self.entry.id, &self.entry.action_descriptors).expect("declared actions")
        }

        fn state(&self) -> Box<dyn GpuSimState> {
            self.seeded(1)
        }

        fn seeded(&self, seed: u64) -> Box<dyn GpuSimState> {
            let Ok(ModelState::Gpu(state)) = (self.entry.create)(&self.params, Some(seed)) else {
                panic!("{} builds as a GPU model", self.entry.id);
            };
            state
        }

        /// Returns the stats export's rows for `total` ticks of `state`, without the header.
        fn rows(&self, state: Box<dyn GpuSimState>, every: u64, schedule: &Schedule, total: u64) -> Vec<String> {
            let every = every.to_string();
            let args = Args::parse_from(["henad-cli", self.entry.id.as_str(), "--stats-every", every.as_str()]);
            let mut out = Vec::new();
            stats_gpu(state, &self.ctx, &args, schedule, total, StatsWriter::new(&mut out)).expect("writes");
            let text = String::from_utf8(out).expect("utf-8");
            text.lines().skip(1).map(str::to_owned).collect()
        }
    }

    /// Checks that a GPU action on a sampling boundary fires once. It used to fire at the end of one run of
    /// steps and again at the start of the next, so the series changed with `--stats-every`.
    ///
    /// `randomise` draws a fresh board on every press. At `--stats-every 1` tick 2 is a boundary, and a
    /// second press there changed every later row. At `--stats-every 3` it is inside a run.
    #[test]
    fn a_gpu_action_fires_once_whatever_the_sampling_interval() {
        let Some(life) = SmallGpuGrid::new("gpu_game_of_life") else {
            return;
        };
        let schedule = life.schedule(&["randomise@2"]);
        let every_tick = life.rows(life.state(), 1, &schedule, 6);
        let every_third = life.rows(life.state(), 3, &schedule, 6);
        assert_eq!(every_tick.len(), 7, "ticks 0 to 6");
        let shared = [0, 3, 6].map(|tick| every_tick[tick].clone());
        assert_eq!(every_third, shared, "rows at ticks 0, 3 and 6");
    }

    /// Checks that a GPU benchmark rep fires each action once and on its own tick, the tick the rep stops on
    /// included.
    ///
    /// The rep is [`run_gpu_rep`], the one `bench_gpu` runs. Its counts have to match the last row of a stats
    /// export over the same ticks. `seed_outbreak` infects a share of the cells still susceptible, so a press
    /// missed, repeated or moved to another tick changes the counts.
    #[test]
    fn a_gpu_benchmark_rep_fires_every_action_once() {
        let Some(sir) = SmallGpuGrid::new("gpu_sir") else {
            return;
        };
        let schedule = sir.schedule(&[
            "seed_outbreak@0",
            "seed_outbreak@2",
            "seed_outbreak@3",
            "seed_outbreak@5",
        ]);
        let no_actions = sir.schedule(&[]);
        for (warmup, steps) in [(0, 0), (0, 3), (2, 0), (2, 3)] {
            let mut rep = sir.state();
            run_gpu_rep(&mut *rep, &sir.ctx, warmup, steps, &schedule).expect("a rep");
            let after_rep = sir.rows(rep, 1, &no_actions, 0);
            let exported = sir.rows(sir.state(), 1, &schedule, warmup + steps);
            assert_eq!(after_rep.last(), exported.last(), "--warmup {warmup} --steps {steps}");
        }
    }
    /// Checks that a GPU sweep of one run writes the series `--export-stats` writes for the run's seed.
    #[test]
    fn a_gpu_single_point_sweep_matches_export_stats() {
        let Some(sir) = SmallGpuGrid::new("gpu_sir") else {
            return;
        };
        let dir = ScratchDir::new("gpu-single-point");
        let args = Args::parse_from([
            "henad-cli",
            "gpu_sir",
            "--set",
            "grid_width=64",
            "--set",
            "grid_height=64",
            "--steps",
            "13",
            "--stats-every",
            "4",
            "--seed",
            "7",
            "--out",
            dir.arg(),
        ]);
        let status = explore::run(&args, &sir.entry, Some(&sir.ctx), None, None).expect("the sweep runs");
        assert_eq!(status, ExitCode::SUCCESS);

        let exported = sir.rows(sir.seeded(run_seed(7, 0)), 4, &sir.schedule(&[]), 13);
        let series = series_without_run_id(&dir);
        assert_eq!(series[1..], exported, "rows at ticks 0, 4, 8, 12 and 13");
        assert_final_reducers(&dir, &series[0], &exported[exported.len() - 1]);
    }
}
