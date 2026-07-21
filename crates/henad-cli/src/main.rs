//! `henad-cli` — a headless benchmark runner for Henad models.
//!
//! This is the non-GUI sibling of `henad-app`: it instantiates a model straight from the shared
//! [`model_registry`] and steps its `SimState` in a bare loop, with no rendering, no `SimThread`,
//! and no pacing. That deliberate leanness is the point — `SimThread`'s EMA smoothing, snapshot
//! throttling, and TPS capping all exist to keep a *UI* responsive, and every one of them is
//! measurement noise for a benchmark. Here we time nothing but `state.step()`.
//!
//! Both CPU and GPU models run. GPU support needs a `wgpu::Device`, which `henad-compute` never
//! creates itself — so this binary acquires one headlessly (no window, no surface; see
//! [`acquire_gpu`]) and hands the resulting [`GpuContext`] to [`model_registry`]. If no device is
//! available, the registry falls back to CPU-only. GPU stepping deliberately does *not* go through
//! `SimState::step()` (one unwaited submission per step); it batches steps and blocks on GPU
//! completion — see [`run_gpu_steps`].
//!
//! ```text
//! henad-cli --list
//! henad-cli game_of_life --steps 10000 --reps 5
//! henad-cli sir --set grid_width=512 --steps 2000 --export final.txt
//! henad-cli gpu_game_of_life --set grid_width=4096 --set grid_height=4096 --steps 10000
//! ```

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "henad-cli is a command-line tool: stdout carries its results, stderr its progress log"
)]

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use clap::Parser;

use henad_compute::gpu::{GpuContext, GpuSimState};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_models::registry::{ModelEntry, ModelState, model_registry};
use numfmt::{Formatter, Scales};

/// Headless benchmark runner for Henad models.
#[derive(Parser)]
#[command(name = "henad-cli", version, about)]
struct Args {
    /// Model id to benchmark (see `--list`).
    #[arg(required_unless_present = "list")]
    model: Option<String>,

    /// Steps to run (and time) per rep.
    #[arg(long, default_value_t = 1000)]
    steps: u64,

    /// Untimed steps run before each timed rep, on that rep's own state, to reach a steady sim regime.
    #[arg(long, default_value_t = 0)]
    warmup: u64,

    /// Untimed steps for a one-time hardware warm-up ("rep 0") before the timed reps — ramps GPU
    /// clocks and pays first-use compilation so rep 1 isn't cold. Its cost scales with the workload.
    #[arg(long = "global-warmup", default_value_t = 0)]
    global_warmup: u64,

    /// How many independent timed runs to collect (each on a freshly created state).
    #[arg(long, default_value_t = 1)]
    reps: usize,

    /// Override a model parameter, e.g. `--set grid_size=512`. Repeatable.
    #[arg(long = "set", value_name = "ID=VALUE")]
    set: Vec<String>,

    /// Write the final state (after warmup + steps) to this path, then exit.
    #[arg(long, value_name = "PATH")]
    export: Option<PathBuf>,

    /// List available models and exit.
    #[arg(long)]
    list: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Best-effort headless GPU: acquire a device so GPU models can be listed and run. If none is
    // available (e.g. CI with no GPU), fall back to a CPU-only registry rather than failing.
    let gpu_ctx = match acquire_gpu() {
        Ok(ctx) => Some(ctx),
        Err(err) => {
            eprintln!("note: no GPU available ({err}); GPU models disabled");
            None
        }
    };
    let registry = model_registry(gpu_ctx.clone());

    if args.list {
        print_models(&registry);
        return Ok(());
    }

    let model_id = args.model.as_deref().context("a model id is required (try --list)")?;
    let entry = registry
        .iter()
        .find(|e| e.id == model_id)
        .with_context(|| format!("unknown model '{model_id}' (try --list)"))?;

    let overrides = parse_overrides(&args.set)?;
    let params = resolve_params(&entry.param_descriptors, &overrides)?;

    if let Some(path) = &args.export {
        return export_final(entry, &params, &args, path);
    }

    run_benchmark(entry, &params, &args, gpu_ctx.as_ref())
}

/// Print every registered model's id and human name.
fn print_models(registry: &[ModelEntry]) {
    println!("available models:");
    for entry in registry {
        let (id, name) = (&entry.id, &entry.name);
        println!("  {id:<18} {name}");
    }
}

/// Create a fresh CPU state from a registry entry. Errors on a GPU-backed model — callers that can
/// handle GPU use [`new_gpu_state`] instead; the dispatcher in [`run_benchmark`] routes correctly,
/// so this only fires for a CPU-only path handed a GPU model (e.g. `--export`, which has no GPU
/// readback).
fn new_cpu_state(entry: &ModelEntry, params: &[ParamValue]) -> Result<Box<dyn SimState>> {
    match (entry.create)(params) {
        ModelState::Cpu(state) => Ok(state),
        ModelState::Gpu(_) => bail!("model '{}' is GPU-backed; this path is CPU-only", entry.id),
    }
}

/// Dispatch to the CPU or GPU benchmark depending on which backend the registry entry produces.
/// The probe state created here is thrown away; each per-rep loop builds its own fresh state.
fn run_benchmark(entry: &ModelEntry, params: &[ParamValue], args: &Args, gpu_ctx: Option<&GpuContext>) -> Result<()> {
    match (entry.create)(params) {
        ModelState::Cpu(_) => bench_cpu(entry, params, args),
        ModelState::Gpu(_) => {
            let ctx = gpu_ctx.context("GPU model selected but no GPU device is available")?;
            bench_gpu(entry, params, args, ctx)
        }
    }
}

/// CPU benchmark: for each rep, build a fresh state, warm it up untimed, then time `steps` steps.
/// Timing wraps *only* the step loop, so state construction and warmup allocation stay out of the
/// measured window.
fn bench_cpu(entry: &ModelEntry, params: &[ParamValue], args: &Args) -> Result<()> {
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
        let mut warm = new_cpu_state(entry, params)?;
        eprint!("  #{: >4}: ", 0);
        let start = Instant::now();
        for _ in 0..args.global_warmup {
            warm.step();
        }
        let elapsed = start.elapsed();
        eprintln!("{elapsed:>8.3?}  ({0} global warmup steps)", args.global_warmup);
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(args.reps);
    let mut population: u64 = 0;
    // Grid dimensions read from the running state, not the params: the state is authoritative —
    // it reflects any `--set grid_width=…` override and whatever the model actually built.
    let mut grid_dims: Option<(u32, u32)> = None;

    for rep in 0..args.reps {
        let mut state = new_cpu_state(entry, params)?;
        for _ in 0..args.warmup {
            state.step();
        }
        // For grid models `population()` is the total cell count (width×height); for agent models
        // it is the agent count. Either way it is the right denominator for agent-updates/sec.
        population = state.population();
        grid_dims = state.grid_view().map(|g| (g.width, g.height));

        let start = Instant::now();
        eprint!("  #{: >4}: ", rep + 1);
        for _ in 0..args.steps {
            state.step();
        }
        let elapsed = start.elapsed();
        eprintln!("{elapsed:>8.3?}");
        samples.push(elapsed);
    }

    report(&samples, args.steps, population, grid_dims)?;
    Ok(())
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
        sorted[sorted.len() / 2]
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

/// GPU benchmark. GPU state never leaves the device, so we cannot walk it via `SimState::step()`
/// (that submits one tiny command buffer per step and never waits). Each rep instead batches
/// `steps` into GPU submissions and blocks on completion — see [`run_gpu_steps`].
fn bench_gpu(entry: &ModelEntry, params: &[ParamValue], args: &Args, ctx: &GpuContext) -> Result<()> {
    eprintln!(
        "benchmarking {} ({}) [GPU]: {} steps x {} reps, {} warmup, {} global-warmup",
        entry.name, entry.id, args.steps, args.reps, args.warmup, args.global_warmup
    );
    if cfg!(debug_assertions) {
        eprintln!("!!! warning: debug build; use --release for benchmarking !!!");
    }

    // Rep 0: optional one-time hardware warm-up. Steps a throwaway state to ramp the GPU off its
    // idle clocks (DVFS) and pay first-use shader compilation before any timed rep — so rep 1 isn't
    // cold. Off by default because its cost scales with the workload; opt in with `--global-warmup`.
    if args.global_warmup > 0 {
        let mut warm = new_gpu_state(entry, params)?;
        eprint!("  #{: >4}: ", 0);
        let start = Instant::now();
        run_gpu_steps(&mut *warm, ctx, args.global_warmup)?;
        let elapsed = start.elapsed();
        eprintln!("{elapsed:>8.3?}  ({0} global warmup steps)", args.global_warmup);
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(args.reps);
    let mut population: u64 = 0;

    for rep in 0..args.reps {
        let mut state = new_gpu_state(entry, params)?;
        // Per-rep sim warm-up (untimed), matching the CPU path.
        run_gpu_steps(&mut *state, ctx, args.warmup)?;
        population = state.population();

        eprint!("  #{: >4}: ", rep + 1);
        let start = Instant::now();
        run_gpu_steps(&mut *state, ctx, args.steps)?;
        let elapsed = start.elapsed();
        eprintln!("{elapsed:>8.3?}");
        samples.push(elapsed);
    }

    // GPU state exposes no `grid_view()`, so derive dimensions from the resolved params instead —
    // which are exactly what the model was built from.
    let grid_dims = grid_dims_from_params(&entry.param_descriptors, params);
    report(&samples, args.steps, population, grid_dims)?;
    Ok(())
}

/// Create a fresh GPU state from a registry entry. Errors on a CPU-backed model.
fn new_gpu_state(entry: &ModelEntry, params: &[ParamValue]) -> Result<Box<dyn GpuSimState>> {
    match (entry.create)(params) {
        ModelState::Gpu(state) => Ok(state),
        ModelState::Cpu(_) => bail!("expected a GPU model but '{}' is CPU-backed", entry.id),
    }
}

/// Run `count` steps on the GPU, blocking until the GPU has actually finished all of them.
///
/// This is the one spot where GPU benchmarking is easy to get *wrong*: `queue.submit()` returns
/// before the GPU has executed anything, so a timer wrapped around submission alone measures
/// CPU-side dispatch cost and reports throughput that looks fantastic and is fiction. The work has
/// to reach the GPU and complete inside the timed window.
fn run_gpu_steps(state: &mut dyn GpuSimState, ctx: &GpuContext, count: u64) -> Result<()> {
    if count == 0 {
        return Ok(());
    }
    // One compute pass is recorded per step (the ping-pong buffers need a pass boundary each step),
    // so encoding all `count` steps into a single command buffer becomes pathological — on Metal,
    // thousands of passes in one buffer stall for minutes. Chunk into fixed-size submissions
    // instead. Correctness is unchanged: wgpu executes submissions to one queue in order, each
    // atomic, so batch N+1 still reads batch N's output. The batches queue up and one final wait
    // drains them all, which also lets the CPU encode ahead of the GPU (pipelining).
    const BATCH: u32 = 256;
    let mut remaining = count;
    while remaining > 0 {
        let n = remaining.min(u64::from(BATCH)) as u32;
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("cli_gpu_steps"),
        });
        state.encode_steps(&mut encoder, n, None);
        ctx.queue.submit(Some(encoder.finish()));
        remaining -= u64::from(n);
    }
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("GPU failed to complete steps")?;
    Ok(())
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

/// Acquire a headless GPU device — the same thing eframe does for henad-app, minus any window or
/// surface. `henad-compute` deliberately never creates a device, so a non-GUI runner must.
fn acquire_gpu() -> Result<GpuContext> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .context("no suitable GPU adapter found")?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("henad-cli"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .context("failed to create GPU device")?;
    // No surface exists, so `target_format` is arbitrary: the models' display texture is an
    // offscreen Rgba8Unorm target, never a swapchain, and the benchmark never reads it back.
    Ok(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
}

/// Run once (warmup + steps) and write the final state to `path`.
fn export_final(entry: &ModelEntry, params: &[ParamValue], args: &Args, path: &Path) -> Result<()> {
    let mut state = new_cpu_state(entry, params)?;
    for _ in 0..(args.warmup + args.steps) {
        state.step();
    }
    write_state(&*state, path)?;
    eprintln!("exported final state (tick {}) to {}", state.tick(), path.display());
    Ok(())
}

/// Serialize a CPU model's view to a simple text format: a grid as comma-separated cell indices
/// per row, a point cloud as `x,y` CSV.
fn write_state(state: &dyn SimState, path: &Path) -> Result<()> {
    let file = File::create(path).with_context(|| format!("could not create '{}'", path.display()))?;
    let mut out = BufWriter::new(file);

    if let Some(grid) = state.grid_view() {
        writeln!(out, "# grid {}x{}", grid.width, grid.height)?;
        for row in grid.cells.chunks(grid.width as usize) {
            let line = row.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
            writeln!(out, "{line}")?;
        }
    } else if let Some(points) = state.point_view() {
        let n = points.pos_x.len();
        writeln!(out, "# points {n}")?;
        writeln!(out, "x,y")?;
        for (x, y) in points.pos_x.iter().zip(points.pos_y) {
            writeln!(out, "{x},{y}")?;
        }
    } else {
        bail!("model exposes no CPU-side view to export");
    }

    Ok(())
}

/// Split each raw `--set ID=VALUE` string into an `(id, value)` pair.
fn parse_overrides(raw: &[String]) -> Result<Vec<(String, String)>> {
    raw.iter()
        .map(|s| {
            let (id, value) = s
                .split_once('=')
                .with_context(|| format!("bad --set '{s}', expected ID=VALUE"))?;
            Ok((id.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Start from every parameter's default, then apply the overrides by id.
fn resolve_params(descriptors: &[ParamDescriptor], overrides: &[(String, String)]) -> Result<Vec<ParamValue>> {
    let mut values: Vec<ParamValue> = descriptors.iter().map(|d| d.kind.default_value()).collect();

    for (id, raw) in overrides {
        let index = descriptors
            .iter()
            .position(|d| d.id == *id)
            .with_context(|| format!("model has no parameter '{id}'"))?;
        values[index] = parse_value(&descriptors[index].kind, raw)?;
    }

    Ok(values)
}

/// Parse a raw string into a [`ParamValue`] matching the descriptor's kind.
fn parse_value(kind: &ParamKind, raw: &str) -> Result<ParamValue> {
    let value = match kind {
        ParamKind::F32 { .. } => ParamValue::F32(raw.parse().with_context(|| format!("'{raw}' is not a number"))?),
        ParamKind::U32 { .. } => ParamValue::U32(raw.parse().with_context(|| format!("'{raw}' is not an integer"))?),
        ParamKind::Bool { .. } => ParamValue::Bool(raw.parse().with_context(|| format!("'{raw}' is not a bool"))?),
        ParamKind::Choice { options, .. } => {
            // Accept either a numeric index or one of the option labels.
            if let Ok(index) = raw.parse::<usize>() {
                ParamValue::Choice(index)
            } else {
                let index = options
                    .iter()
                    .position(|o| *o == raw)
                    .with_context(|| format!("'{raw}' is not one of {options:?}"))?;
                ParamValue::Choice(index)
            }
        }
    };
    Ok(value)
}
