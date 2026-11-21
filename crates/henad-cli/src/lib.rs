//! The command line of Henad, a headless benchmark and sweep runner, as a library.
//!
//! [`run`] parses a command line, runs it over the models a [`CliOptions`] holds, and returns the exit code. The
//! official `henad-cli` binary is three lines over it with the example models. A project with its own models builds
//! the same command line over its own [`ModelSet`].
//!
//! ```no_run
//! use std::process::ExitCode;
//!
//! use henad_cli::CliOptions;
//! use henad_compute::entry::ModelSet;
//!
//! fn main() -> ExitCode {
//!     let models = ModelSet::new(henad_core::build_info!());
//!     let options = CliOptions::new(models, henad_core::build_info!()).command_name("my-models");
//!     ExitCode::from(henad_cli::run(options, std::env::args_os()))
//! }
//! ```
//!
//! A benchmark builds a model from the set and steps its `SimState` in a bare loop, with no rendering, no `SimThread`
//! and no pacing, so a measurement times nothing but `state.step()`.
//!
//! Both CPU and GPU models run. For a set that holds a GPU model, [`run`] acquires a device through
//! [`acquire_headless`], sized to the set's needs, and builds GPU models on the resulting [`GpuContext`]. Without a
//! device `--list` leaves the GPU models out, and a GPU model id on the command line is rejected. The benchmark is
//! [`henad_explore::benchmark::run_benchmark`], and the two exports step a
//! [`Simulation`](henad_compute::simulation::Simulation).
//!
//! `--model-file` reads a model file and adds its model to the set, in place of a model with the same id. A model file
//! holds a grid rule (`kind = "gpu_grid"`), a graph model (`kind = "graph"`) or a sheet (`kind = "sheet"`). The files
//! and `--spec` are read before the device is acquired. The device is then sized to the files' models as well. A graph
//! reads, checks and compiles to WGSL, on a grid or of agents, and its warnings and errors print at their lines. A
//! sheet reads, checks and lowers to a graph, which then builds as a graph does, and each of its warnings and errors
//! prints at the line of the sheet it is about, with no pass and no node. `--print-lowered` prints the graph that one
//! model file builds as, a sheet's lowered graph included, in its canonical text.
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
//! henad-cli --model-file crates/henad-explore/specs/models/life_rule.toml --vary density=0.1:0.5:0.1 --out life-sweep
//! henad-cli sheet_game_of_life --model-file crates/henad-graph/sheets/game_of_life.toml --steps 100
//! henad-cli --model-file crates/henad-graph/sheets/game_of_life.toml --print-lowered
//! henad-cli --merge shard-0 shard-1 --out sir-sweep
//! ```
//!
//! `--export` writes the *final state*, the grid or point cloud at the end of the run. `--export-stats` writes the
//! *time series*, one row per sampled tick. Both formats live in `henad_core::export`, and the app writes through the
//! same module.
//!
//! `--out`, `--spec` or `--dry-run` runs a sweep instead, many runs over a grid of parameter values written to a
//! directory, and `--merge` joins a sweep's shard directories.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "henad-cli is a command line: stdout carries its results, stderr its progress log"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use clap::error::ErrorKind;
use clap::{ArgGroup, CommandFactory as _, FromArgMatches as _, Parser};

use henad_compute::entry::{ModelEntry, ModelLookupError, ModelSet};
use henad_compute::fault::install_panic_hook;
use henad_compute::gpu::GpuContext;
use henad_compute::runtime_info::{GpuVerdict, HostInfo, RuntimeInfo, classify_adapter};
use henad_core::params::{ParamFormat, ParamKind};
use henad_core::provenance::BuildInfo;
use henad_explore::device::acquire_headless;
use henad_explore::model_file::ModelFile;
use henad_explore::spec_file::{LoadedSpec, SpecFileError};
use henad_explore::sweep::Provenance;

use crate::explore::ExploreArgs;
use crate::model_file::{print_lowered, with_model_files};
use crate::single::{SingleRun, run_single};

mod explore;
mod json_report;
mod model_file;
mod single;

/// Options for the command line that a host runs: the models, the host's build, the command name and the help text's
/// opening line.
#[derive(Debug, Clone)]
pub struct CliOptions {
    models: ModelSet,
    /// Build of the host, recorded in every sweep's manifest.
    host: BuildInfo,
    /// Command name that `--version`, the help text and the usage lines show.
    command_name: String,
    /// Line that the help text opens with, or `None` for henad-cli's own description.
    about: Option<String>,
}

impl CliOptions {
    /// Returns options that run `models` and record `host` as the build that ran each sweep.
    ///
    /// The command name defaults to the package name `host` records, and `--version` prints its version. The help
    /// text opens with henad-cli's own description until [`Self::about`] replaces it.
    pub fn new(models: ModelSet, host: BuildInfo) -> Self {
        Self {
            command_name: host.package().to_owned(),
            models,
            host,
            about: None,
        }
    }

    /// Sets the command name that `--version`, the help text and the usage lines show.
    ///
    /// The usage lines use this name instead of the program name that the command line starts with.
    pub fn command_name(mut self, name: impl Into<String>) -> Self {
        self.command_name = name.into();
        self
    }

    /// Sets the line that the help text opens with.
    pub fn about(mut self, text: impl Into<String>) -> Self {
        self.about = Some(text.into());
        self
    }
}

/// Exit code of a sweep that ran to its end with a run that is not `ok`, or of a merge that is missing a run or holds a
/// run that is not `ok`.
pub const SOME_RUNS_NOT_OK: u8 = 3;

/// Parses `arguments`, the program name first, runs them over the options' models, and returns the exit code.
///
/// The code is 0 on success, [`SOME_RUNS_NOT_OK`] for a sweep or merge with a missing run or a run that is not `ok`,
/// and 2 for a command line that does not parse. Any other error prints to stderr as `Error:` and its causes, and
/// returns 1.
///
/// Note that this owns the process. It installs the panic hook, and `--threads` sizes rayon's global pool. A process
/// can size that pool only once.
pub fn run(options: CliOptions, arguments: impl IntoIterator<Item = OsString>) -> u8 {
    install_panic_hook();
    let CliOptions {
        models,
        host,
        command_name,
        about,
    } = options;
    let arguments: Vec<OsString> = arguments.into_iter().collect();
    let args = match parse_args(command_name, host.version(), about, &arguments) {
        Ok(args) => args,
        Err(error) => {
            // Help and the version go to stdout with code 0, a usage error to stderr with code 2.
            error.print().ok();
            return u8::try_from(error.exit_code()).unwrap_or(1);
        }
    };
    match run_args(&models, &host, &args, &arguments) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:?}");
            1
        }
    }
}

/// Parses `arguments` as the command `name` at `version`, whose help text opens with `about` when it is set.
///
/// # Errors
///
/// Returns clap's error for a command line that does not parse, for `--help` and `--version`, and for two model files
/// or more with neither a model id nor a spec in a mode that needs a model.
fn parse_args(
    name: String,
    version: &'static str,
    about: Option<String>,
    arguments: &[OsString],
) -> Result<Args, clap::Error> {
    // Without `bin_name`, clap names the command in usage lines after the program name.
    let mut command = Args::command().name(name.clone()).bin_name(name).version(version);
    if let Some(about) = about {
        command = command.about(about);
    }
    let mut matches = command.try_get_matches_from_mut(arguments)?;
    let args = Args::from_arg_matches_mut(&mut matches).map_err(|error| error.format(&mut command))?;
    if args.print_lowered && args.model_file.len() != 1 {
        return Err(command.error(
            ErrorKind::WrongNumberOfValues,
            "--print-lowered requires exactly one --model-file",
        ));
    }
    // Clap accepts any number of model files in place of the model id. Only one of them can stand in for it.
    let needs_model = !matches!(
        Mode::of(&args),
        Mode::List | Mode::Merge | Mode::InfoOnly | Mode::PrintLowered
    );
    if needs_model && args.model.is_none() && args.explore.spec.is_none() && args.model_file.len() > 1 {
        return Err(command.error(
            ErrorKind::MissingRequiredArgument,
            "a model id is required with more than one --model-file",
        ));
    }
    Ok(args)
}

/// Headless benchmark runner for Henad models.
#[derive(Parser)]
#[command(about)]
#[command(group(
    ArgGroup::new("explore")
        .args(["out", "spec", "dry_run"])
        .multiple(true)
        .conflicts_with_all(["list", "export", "export_stats", "global_warmup"])
))]
// A spec file needs one of these flags. Each flag puts the spec to a use.
#[command(group(ArgGroup::new("spec_use").args(["out", "dry_run", "params"]).multiple(true)))]
struct Args {
    /// Model id to run (see `--list`). Optional with `--spec`, and with one `--model-file`.
    #[arg(required_unless_present_any = ["list", "info", "spec", "merge", "model_file"])]
    model: Option<String>,

    /// Load a model file and add its model to this build's models, replacing a model with the same id. Repeatable.
    #[arg(long = "model-file", value_name = "PATH")]
    model_file: Vec<PathBuf>,

    /// Print the canonical text of the graph that the single `--model-file` builds, and exit. For a sheet, print the
    /// lowered graph.
    #[arg(
        long = "print-lowered",
        requires = "model_file",
        conflicts_with_all = [
            "model", "list", "params", "info", "json", "export", "export_stats", "stats_every", "merge", "explore",
            "steps", "warmup", "global_warmup", "seed", "reps", "set", "act", "threads",
        ]
    )]
    print_lowered: bool,

    /// Steps to run (and time) per rep, or per run of a sweep.
    #[arg(long, default_value_t = 1000)]
    steps: u64,

    /// Untimed steps run before each timed rep, on that rep's own state, to reach a steady sim regime.
    #[arg(long, default_value_t = 0)]
    warmup: u64,

    /// Untimed steps for a one-time hardware warm-up before the timed reps. The warm-up ramps up GPU clocks and
    /// triggers first-use compilation, so rep 1 is not cold. Its cost scales with the workload.
    #[arg(long = "global-warmup", default_value_t = 0)]
    global_warmup: u64,

    /// RNG seed used. A sweep derives each run's seed from it and uses 0 by default.
    #[arg(long)]
    seed: Option<u64>,

    /// Independent timed runs to collect, each on a freshly created state. A sweep runs this many replicates of
    /// each config.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
    reps: u64,

    /// Override a model parameter, e.g. `--set grid_width=512`. Repeatable.
    #[arg(long = "set", value_name = "ID=VALUE", value_parser = parse_set)]
    set: Vec<(String, String)>,

    /// Run one of the model's actions at a tick, e.g. `--act clear@500`. Repeatable. An unknown id is rejected, and the
    /// error lists the ids the model declares. Each rep and each run of a sweep replays the same schedule. A sweep
    /// names the action by its id, or `ID_2` for the second `--act` of an id, skipping a name an earlier `--act` has
    /// taken.
    #[arg(long = "act", value_name = "ID@TICK", value_parser = check_act)]
    act: Vec<String>,

    /// Write the final state (after warmup + steps) to this path, then exit.
    #[arg(long, value_name = "PATH")]
    export: Option<PathBuf>,

    /// Write the per-tick stat time series to this path as CSV, then exit. One configuration runs for `warmup + steps`
    /// steps, with stats sampled every `--stats-every` ticks.
    #[arg(long = "export-stats", value_name = "PATH")]
    export_stats: Option<PathBuf>,

    /// Sample stats every N ticks when using `--export-stats` or sweeping. Set to 1 to record every tick.
    #[arg(long = "stats-every", default_value_t = 1, value_name = "N")]
    stats_every: u64,

    /// List available models and exit.
    #[arg(long)]
    list: bool,

    /// Print the model's parameters (the ids `--set` takes, with kinds and defaults) and exit. With `--json`,
    /// print them as one JSON object with the model's stats and actions.
    #[arg(long, conflicts_with_all = ["out", "dry_run"])]
    params: bool,

    /// Print host and GPU information. With no model and no sweep, print it and exit. Otherwise print it as a
    /// provenance header before the benchmark or sweep.
    #[arg(long)]
    info: bool,

    /// Emit one JSON object per line instead of the human report, for a driver to parse.
    #[arg(long)]
    json: bool,

    /// Worker threads for CPU models. Set to 0 to use rayon's default of one thread per logical CPU.
    #[arg(long, default_value_t = 0, value_name = "N")]
    threads: usize,

    #[command(flatten)]
    explore: ExploreArgs,
}

/// Work that one command line requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode<'a> {
    /// `--print-lowered`. Prints the graph of one model file, with no model set and no device.
    PrintLowered,
    List,
    /// `--merge`. Joins a sweep's shard directories, with no model and no device.
    Merge,
    /// `--info` with no model and no sweep. Prints the runtime and exits. A single `--model-file` names a model.
    InfoOnly,
    Params,
    Explore,
    /// One configuration of a model, run outside a sweep.
    Single(SingleRun<'a>),
}

impl<'a> Mode<'a> {
    /// Returns the mode of `args`, using the first variant that applies in declaration order.
    fn of(args: &'a Args) -> Self {
        if args.print_lowered {
            Self::PrintLowered
        } else if args.list {
            Self::List
        } else if !args.explore.merge.is_empty() {
            Self::Merge
        } else if args.info && args.model.is_none() && args.model_file.len() != 1 && !args.explore.is_sweep() {
            Self::InfoOnly
        } else if args.params {
            Self::Params
        } else if args.explore.is_sweep() {
            Self::Explore
        } else if let Some(path) = &args.export_stats {
            Self::Single(SingleRun::ExportStats(path))
        } else if let Some(path) = &args.export {
            Self::Single(SingleRun::ExportFinal(path))
        } else {
            Self::Single(SingleRun::Benchmark)
        }
    }
}

/// Runs the command line `args`, parsed from `arguments`, over `models`, and returns the exit code.
///
/// A sweep records `host` as the build that ran it.
///
/// # Errors
///
/// Returns an error when the command line cannot run.
fn run_args(models: &ModelSet, host: &BuildInfo, args: &Args, arguments: &[OsString]) -> Result<u8> {
    let mode = Mode::of(args);

    // The pool is sized before anything builds a state. Rayon's global pool is set once per process, and
    // `HostInfo::worker_threads` reads back whatever it ends up with.
    if args.threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.threads)
            .build_global()
            .context("cannot set the number of worker threads")?;
    }
    if mode == Mode::Merge {
        return explore::merge_shards(args);
    }
    if let (Mode::PrintLowered, [path]) = (mode, args.model_file.as_slice()) {
        return print_lowered(path);
    }

    // The model files and the spec are read before the device is acquired, so the device is sized to every model the
    // command line can name.
    let (models, files) = with_model_files(models, &args.model_file, !args.json)?;
    let models = &*models;
    let spec = args.explore.spec.as_deref().map(load_spec).transpose()?;

    // A device lets the GPU models be listed and run. Without a device, as on CI with no GPU, only the CPU models are
    // listed and run. A set without GPU models requests a device only under `--info`, to report its adapter.
    let gpu_models = has_gpu_models(models);
    let gpu_ctx = if gpu_models || args.info {
        headless_device(models, gpu_models)
    } else {
        None
    };
    let runtime = gpu_ctx.as_ref().and_then(GpuContext::runtime_info);

    if args.info {
        if args.json {
            json_report::runtime(runtime);
        } else {
            print_runtime_info(runtime, gpu_models);
        }
    }

    match mode {
        Mode::List => {
            print_models(models.runnable(gpu_ctx.as_ref()));
            return Ok(0);
        }
        Mode::InfoOnly => return Ok(0),
        Mode::PrintLowered | Mode::Merge | Mode::Params | Mode::Explore | Mode::Single(_) => {}
    }

    let model_id = model_id(args, spec.as_ref(), &files)?;
    let entry = models.lookup(model_id, gpu_ctx.as_ref()).map_err(|error| match error {
        ModelLookupError::NotInSet { .. } => anyhow!("{error} (try --list)"),
        _ => anyhow!(error),
    })?;

    let file = files.iter().find(|file| file.id() == entry.id());
    match mode {
        Mode::Params if args.json => json_report::emit(&json_report::params(entry, gpu_ctx.as_ref())),
        Mode::Params => print!("{}", params_text(entry)),
        Mode::Explore => {
            let provenance = provenance(host, arguments);
            return explore::run(args, entry, file, gpu_ctx.as_ref(), spec, provenance);
        }
        Mode::Single(run) => return run_single(entry, file, args, run, gpu_ctx.as_ref(), runtime).map(|()| 0),
        // Each of these returned above.
        Mode::PrintLowered | Mode::List | Mode::Merge | Mode::InfoOnly => {}
    }
    Ok(0)
}

/// Returns a headless device sized to the needs of `models`, or `None` when this machine has no device.
///
/// `gpu_models` says whether `models` holds a GPU model. Only then does a missing device print a note on stderr, and
/// a software rasteriser a warning.
fn headless_device(models: &ModelSet, gpu_models: bool) -> Option<GpuContext> {
    let ctx = match acquire_headless(models.gpu_needs()) {
        Ok(ctx) => ctx,
        Err(err) => {
            if gpu_models {
                eprintln!("note: no GPU available ({err}), so GPU models are disabled");
            }
            return None;
        }
    };
    // `force_fallback_adapter: false` does not stop a software rasteriser (lavapipe, WARP) being
    // returned when it is the only adapter present.
    if let Some(runtime) = ctx.runtime_info()
        && gpu_models
        && classify_adapter(&runtime.adapter) == GpuVerdict::Absent
    {
        eprintln!(
            "!!! warning: adapter '{}' is a software rasteriser, not a GPU. GPU-model results from this machine are \
                 not GPU results !!!",
            runtime.adapter.name
        );
    }
    Some(ctx)
}

/// Returns the id of the model that the command line names: the model id, else the spec's model, else the id of the
/// model file when `files` holds one.
///
/// # Errors
///
/// Returns an error when the model id differs from the spec's model, and when nothing names a model.
fn model_id<'a>(args: &'a Args, spec: Option<&'a LoadedSpec>, files: &'a [ModelFile]) -> Result<&'a str> {
    Ok(match (args.model.as_deref(), spec, files) {
        (Some(id), Some(loaded), _) if id != loaded.spec.model => {
            bail!("model '{id}' does not match the spec's model '{}'", loaded.spec.model)
        }
        (Some(id), _, _) => id,
        (None, Some(loaded), _) => &loaded.spec.model,
        (None, None, [file]) => file.id(),
        (None, None, []) => bail!("a model id is required (try --list)"),
        // `parse_args` refuses this command line first.
        (None, None, _) => bail!("a model id is required with more than one --model-file"),
    })
}

/// Returns whether `models` holds a GPU model.
fn has_gpu_models(models: &ModelSet) -> bool {
    models.iter().any(|entry| entry.gpu_needs().is_some())
}

/// Returns the builds of Henad and of `host`, with the command line `arguments`.
fn provenance(host: &BuildInfo, arguments: &[OsString]) -> Provenance {
    let arguments = arguments.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
    Provenance::new(*host, arguments)
}

/// Reads the spec file at `path`.
///
/// # Errors
///
/// Returns an error when the file cannot be read, or does not describe a sweep.
fn load_spec(path: &Path) -> Result<LoadedSpec> {
    LoadedSpec::read(path).map_err(|error| match error {
        SpecFileError::Read { .. } => anyhow::Error::new(error),
        other => anyhow::Error::new(other).context(format!("cannot read '{}'", path.display())),
    })
}

/// Prints the host and adapter for `--info` to stdout, ahead of any results.
///
/// `gpu_models` says whether the set holds a GPU model. A missing adapter disables every GPU model.
fn print_runtime_info(runtime: Option<&RuntimeInfo>, gpu_models: bool) {
    let host = runtime.map_or_else(HostInfo::collect, |runtime| runtime.host.clone());

    let fmt_opt = |value: Option<usize>| value.map_or_else(|| "unknown".to_owned(), |n| n.to_string());

    println!("runtime info:");
    println!("  host:");
    println!("    platform:        {} ({})", host.os, host.arch);
    println!("    logical cpus:    {}", fmt_opt(host.logical_cpus));
    println!("    worker threads:  {}", fmt_opt(host.worker_threads));

    match runtime {
        None if gpu_models => println!("  gpu:               none (GPU models disabled)"),
        None => println!("  gpu:               none"),
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

/// Prints the id and human name of each model in `entries`.
fn print_models<'a>(entries: impl Iterator<Item = &'a ModelEntry>) {
    println!("available models:");
    for entry in entries {
        let (id, name) = (entry.id(), entry.name());
        println!("  {id:<18} {name}");
    }
}

/// Returns the text `--params` prints, one line per parameter with the id `--set` accepts, its kind, default and
/// bounds.
///
/// Each line holds `key=value` fields, and a hidden parameter the bare word `hidden` after them.
/// `scripts/bench_matrix.py` parses the fields to find a model's axes and defaults.
fn params_text(entry: &ModelEntry) -> String {
    let mut text = format!("parameters for {} ({}):\n", entry.id(), entry.name());
    for (index, desc) in entry.param_descriptors().iter().enumerate() {
        let (id, label) = (desc.id(), desc.label());
        let apply = if desc.is_live() { "live" } else { "reload" };
        let kind = match desc.kind() {
            ParamKind::F32 { min, max, default, .. } => format!("kind=f32 default={default} min={min} max={max}"),
            ParamKind::U32 { min, max, default } => format!("kind=u32 default={default} min={min} max={max}"),
            ParamKind::Bool { default } => format!("kind=bool default={default}"),
            ParamKind::Choice { options, default } => {
                format!(
                    "kind=choice default={default} options={}",
                    options.iter().collect::<Vec<_>>().join("|")
                )
            }
        };
        // Values are always fractions, whatever the panel shows, so `format` only describes how to read a value.
        let format = match desc.format() {
            ParamFormat::Plain => "",
            ParamFormat::Percent => " format=percent",
        };
        // A hidden parameter is kept off the Parameters panel, and `--set` and a sweep reach it as any other.
        let hidden = if desc.is_hidden() { " hidden" } else { "" };
        text.push_str(&format!(
            "  index={index} id={id} {kind} apply={apply}{format}{hidden} label=\"{label}\"\n"
        ));
    }
    text
}

/// Returns the provenance a test's sweep records, with no command line.
#[cfg(test)]
fn test_provenance() -> Provenance {
    Provenance::new(henad_core::build_info!(), Vec::new())
}

/// Reads `--set ID=VALUE` into an `(id, value)` pair, split at its first `=`.
///
/// The model checks the id and the value once it is known. The parser then rejects a malformed flag.
fn parse_set(raw: &str) -> Result<(String, String), String> {
    let (id, value) = raw.split_once('=').ok_or("expected ID=VALUE")?;
    Ok((id.to_owned(), value.to_owned()))
}

/// Checks that `--act` has the form `ID@TICK` with a `u64` tick, and returns it unchanged.
///
/// The model checks the id once it is known.
fn check_act(raw: &str) -> Result<String, String> {
    let (_, tick) = raw.rsplit_once('@').ok_or("expected ID@TICK")?;
    match tick.parse::<u64>() {
        Ok(_) => Ok(raw.to_owned()),
        Err(_) => Err(format!("expected a tick from 0 to {}, got '{tick}'", u64::MAX)),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{
        Args, CliOptions, Mode, SOME_RUNS_NOT_OK, has_gpu_models, params_text, parse_args, run, test_provenance,
    };
    use crate::explore::{self, ExploreArgs};
    use crate::single::{SingleRun, write_series};

    use clap::Parser as _;
    use henad_compute::entry::{ModelEntry, ModelSet};
    use henad_compute::gpu::GpuContext;
    use henad_compute::simulation::{RunSetup, Simulation};
    use henad_core::action::Schedule;
    use henad_core::explore::seed::run_seed;
    use henad_core::explore::value::{parse_overrides, resolve_params};
    use henad_core::export::csv::parse_records;
    use henad_core::export::stats_csv::StatsWriter;
    use henad_core::params::ParamValue;
    use henad_explore::testing::{TestDeviceRequest, headless_test_device};
    use henad_models::example_models;

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
        example_models().get(id).cloned().expect("the model is registered")
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
    /// `header` holds the column names of `last_row`, as a stats export writes them.
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
    /// 31 steps sampled every 3 ticks end off the sampling boundary, so the final tick gets its own row.
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
        let status = explore::run(&args, &entry, None, None, None, test_provenance()).expect("the sweep runs");
        assert_eq!(status, 0);

        let params = resolve_params(entry.param_descriptors(), &args.set).expect("in range");
        let mut simulation = cpu_simulation(&entry, &params, run_seed(7, 0), &[]);
        let mut exported_bytes = Vec::new();
        write_series(&mut simulation, 31, 3, StatsWriter::new(&mut exported_bytes)).expect("writes");
        let exported = String::from_utf8(exported_bytes).expect("utf-8");
        let exported: Vec<&str> = exported.lines().collect();

        assert_eq!(series_without_run_id(&dir), exported);
        assert_eq!(exported.len(), 1 + 12, "a header, ticks 0 to 30 every 3, and tick 31");
        assert_final_reducers(&dir, exported[0], exported[exported.len() - 1]);
    }

    /// Checks that a sweep fires each `--act` as `--export-stats` fires it, including on the tick that the run ends on.
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
        let status = explore::run(&args, &entry, None, None, None, test_provenance()).expect("the sweep runs");
        assert_eq!(status, 0);

        let params = resolve_params(entry.param_descriptors(), &args.set).expect("in range");
        let mut simulation = cpu_simulation(&entry, &params, run_seed(7, 0), &args.act);
        let mut exported_bytes = Vec::new();
        write_series(&mut simulation, 20, 2, StatsWriter::new(&mut exported_bytes)).expect("writes");
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

    /// Checks that two merged shards of a sampled sweep hold the same files as the sweep run in one go.
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
            explore::run(&args, &entry, None, None, None, test_provenance()).expect("the sweep runs")
        };
        let whole = dir.path.join("whole");
        assert_eq!(sweep(&whole, None), 0);
        let shards = [dir.path.join("shard-0"), dir.path.join("shard-1")];
        for (shard_dir, shard) in shards.iter().zip(["0/2", "1/2"]) {
            assert_eq!(sweep(shard_dir, Some(shard)), 0, "shard {shard}");
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
        assert_eq!(explore::merge_shards(&args).expect("the shards merge"), 0);
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
        assert_eq!(status, SOME_RUNS_NOT_OK, "half the runs are missing");
    }

    /// Checks that [`run`] returns 2 for a command line that does not parse, and 1 for one that cannot run.
    #[test]
    fn run_returns_the_exit_code_of_each_failure() {
        let options = || CliOptions::new(ModelSet::new(henad_core::build_info!()), henad_core::build_info!());
        let line = |arguments: &[&str]| arguments.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(run(options(), line(&["henad-cli", "--no-such-flag"])), 2);
        assert_eq!(
            run(options(), line(&["henad-cli", "--merge", "a"])),
            2,
            "--merge needs --out"
        );
        assert_eq!(
            run(options(), line(&["henad-cli", "--spec", "s.toml"])),
            2,
            "a spec needs --out, --dry-run or --params"
        );
        assert_eq!(
            run(options(), line(&["henad-cli", "sir", "--set", "grid_width"])),
            2,
            "a malformed --set is refused before the model is looked up"
        );
        assert_eq!(
            run(options(), line(&["henad-cli", "sir", "--steps", "1"])),
            1,
            "an empty set holds no sir"
        );
    }

    /// Checks that the usage lines show the command name that the options set, whatever the program name, and that
    /// the help text opens with the host's line, or with henad-cli's description by default.
    #[test]
    fn the_options_name_and_describe_the_command() {
        let render = |about: Option<&str>, arguments: &[&str]| {
            let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
            let about = about.map(str::to_owned);
            let Err(error) = parse_args("my-models".to_owned(), "1.2.3", about, &arguments) else {
                panic!("{arguments:?} ends the parse");
            };
            error.render().to_string()
        };
        let refused = render(None, &["other-name", "--no-such-flag"]);
        assert!(refused.contains("Usage: my-models"), "{refused}");
        let help = render(Some("Runs my models."), &["other-name", "--help"]);
        assert!(help.starts_with("Runs my models."), "{help}");
        assert!(help.contains("Usage: my-models"), "{help}");
        let default = render(None, &["other-name", "--help"]);
        assert!(default.starts_with(env!("CARGO_PKG_DESCRIPTION")), "{default}");
        assert_eq!(render(None, &["other-name", "--version"]), "my-models 1.2.3\n");
    }

    /// Checks that a set holding only CPU models requests no device.
    #[test]
    fn a_cpu_only_set_has_no_gpu_models() {
        let mut cpu_only = ModelSet::new(henad_core::build_info!());
        cpu_only.insert(cpu_entry("sir")).expect("one id");
        assert!(!has_gpu_models(&cpu_only));
        assert!(has_gpu_models(&example_models()));
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

    /// Returns the mode a command line had before sweeps existed.
    fn legacy_mode(args: &Args) -> Mode<'_> {
        if args.list {
            Mode::List
        } else if args.model.is_none() {
            Mode::InfoOnly
        } else if args.params {
            Mode::Params
        } else if let Some(path) = &args.export_stats {
            Mode::Single(SingleRun::ExportStats(path))
        } else if let Some(path) = &args.export {
            Mode::Single(SingleRun::ExportFinal(path))
        } else {
            Mode::Single(SingleRun::Benchmark)
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

    /// Checks that every command line in the docs and scripts keeps the mode it had before sweeps.
    ///
    /// The script lines are written the way `scripts/compare_bench.py`, `bench_matrix.py`, `compare_sir.py` and
    /// `compare_network.py` build them. A documented sweep line enters explore mode, and a merge line merge mode.
    #[test]
    fn existing_invocations_keep_their_mode() {
        // The page is read at run time. It sits outside the package, and a crate built from its tarball skips the test.
        let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/cli.md");
        let Ok(reference) = std::fs::read_to_string(&reference) else {
            eprintln!("note: skipped, {} is absent", reference.display());
            return;
        };
        let mut lines = command_lines(&reference);
        lines.extend(command_lines(include_str!("lib.rs")));
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
            if args.print_lowered {
                // No line written before `--print-lowered` holds the flag.
                assert_eq!(Mode::of(&args), Mode::PrintLowered, "{line:?}");
            } else if args.explore == ExploreArgs::default() {
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
            documented - sweeps >= 13,
            "found {documented} documented lines, {sweeps} of them sweeps"
        );
    }

    /// Returns `entry` built from `params` with `seed` and the `--act` entries `actions`.
    pub(crate) fn cpu_simulation(
        entry: &ModelEntry,
        params: &[ParamValue],
        seed: u64,
        actions: &[String],
    ) -> Simulation {
        let schedule = Schedule::parse(actions, entry.id(), entry.action_descriptors()).expect("declared actions");
        RunSetup::from_parts(entry, params, Some(seed), schedule)
            .expect("the values fit the model")
            .build(None)
            .expect("the model builds")
    }

    /// A 64 by 64 GPU grid model and the device it runs on.
    pub(crate) struct SmallGpuGrid {
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
        pub(crate) fn new(id: &str) -> Option<Self> {
            let ctx = headless_test_device(&TestDeviceRequest::raised(example_models().gpu_needs()))?;
            let entry = example_models().get(id).cloned().expect("the model is registered");
            let overrides = parse_overrides(&["grid_width=64".to_owned(), "grid_height=64".to_owned()]).expect("valid");
            let params = resolve_params(entry.param_descriptors(), &overrides).expect("in range");
            Some(Self { ctx, entry, params })
        }

        pub(crate) fn schedule(&self, raw: &[&str]) -> Schedule {
            let raw: Vec<String> = raw.iter().map(|&s| s.to_owned()).collect();
            Schedule::parse(&raw, self.entry.id(), self.entry.action_descriptors()).expect("declared actions")
        }

        /// Returns the model built with `seed` and `schedule`.
        pub(crate) fn simulation(&self, seed: u64, schedule: &Schedule) -> Simulation {
            RunSetup::from_parts(&self.entry, &self.params, Some(seed), schedule.clone())
                .expect("the values fit the model")
                .build(Some(&self.ctx))
                .expect("the model builds")
        }
    }

    /// Returns the stats export's rows for `total` ticks of `simulation`, without the header.
    pub(crate) fn rows(mut simulation: Simulation, every: u64, total: u64) -> Vec<String> {
        let mut out = Vec::new();
        write_series(&mut simulation, total, every, StatsWriter::new(&mut out)).expect("writes");
        let text = String::from_utf8(out).expect("utf-8");
        text.lines().skip(1).map(str::to_owned).collect()
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
        let status =
            explore::run(&args, &sir.entry, None, Some(&sir.ctx), None, test_provenance()).expect("the sweep runs");
        assert_eq!(status, 0);

        let exported = rows(sir.simulation(run_seed(7, 0), &sir.schedule(&[])), 4, 13);
        let series = series_without_run_id(&dir);
        assert_eq!(series[1..], exported, "rows at ticks 0, 4, 8, 12 and 13");
        assert_final_reducers(&dir, &series[0], &exported[exported.len() - 1]);
    }
}
