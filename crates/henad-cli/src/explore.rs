//! Sweeps and searches from the command line, through `--out`, `--spec` or `--dry-run`, and merges of their shards
//! through `--merge`.
//!
//! Flags build a sweep of one block: a factorial over every `--vary`, a zip with `--zip`, a sample with `--sample`,
//! or a table with `--design`. A spec file gives the whole sweep instead, and conflicts with every flag that changes
//! a result. A spec file with a `[search]` table runs a search. Flags cannot describe one.

use std::fs;
use std::io::{self, IsTerminal as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::action::ScheduleError;
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, FactorTarget, LevelSpec};
use henad_core::explore::fingerprint::fnv1a64;
use henad_core::explore::outcome::{RunOutcome, RunStatus};
use henad_core::explore::plan::{PlannedBlock, Shard};
use henad_core::explore::reducer::ReducerSpec;
use henad_core::explore::search::Objective;
use henad_core::explore::search::pse::PatternAxis;
use henad_core::explore::seed::SeedScheme;
use henad_core::explore::spec::{
    ACTION_COLUMN_PREFIX, ActionSpec, BlockSpec, MeasureSettings, RunSettings, SeedSettings, SweepSpec,
};
use henad_core::explore::stop::StopSpec;
use henad_core::explore::value::parse_overrides;
use henad_explore::exec::{Concurrency, ExecutionLayout};
use henad_explore::handle::SweepOutput;
use henad_explore::merge::{MergeReport, merge};
use henad_explore::output::manifest::ResultCounts;
use henad_explore::progress::{Progress, ProgressEvent, ProgressUpdate};
use henad_explore::schema::backend_name;
use henad_explore::search_run::{SearchOutline, SearchUpdate};
use henad_explore::spec_file::{DesignTableFile, LoadedSpec};
use henad_explore::sweep::{
    Provenance, SpecSource, SweepEnd, SweepOptions, SweepOutline, SweepReport, SweepWarning, plan_spec, run_spec,
};

use crate::Args;
use crate::json_report;

/// Exit status of a sweep that ran to its end with some run not `ok`, or of a merge that lacks some run.
pub const SOME_RUNS_NOT_OK: u8 = 3;

/// Rows of `series.csv` above which the plan carries a warning.
pub const SERIES_ROWS_WARNING: u64 = 10_000_000;

/// Time between two progress lines when stderr is not a terminal.
const LOGGED_PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

/// Flags that make a sweep, or merge the shards of one.
///
/// `--out`, `--spec` and `--dry-run` each ask for a sweep, and every other flag apart from `--merge` needs one of
/// them.
#[derive(clap::Args, Debug, Clone, Default, PartialEq, Eq)]
#[command(next_help_heading = "Sweeps")]
pub struct ExploreArgs {
    /// Run a sweep and write its results to this directory. The directory must hold no results unless `--resume` is
    /// given. With `--merge`, write the merged shards to this directory.
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,

    /// Read the sweep from a TOML spec file. The file names the model and every setting that changes a result. A
    /// `[search]` table in it runs a search instead.
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = [
            "set", "act", "steps", "warmup", "reps", "seed", "stats_every",
            "vary", "zip", "sample", "design_seed", "design", "independent_seeds", "series_every",
            "stop", "reduce", "no_default_reducers", "timeout",
        ]
    )]
    pub spec: Option<PathBuf>,

    /// Vary a parameter over `v1,v2,...`, the inclusive range `min:max:step`, or `all` values of a bool or choice.
    /// The step of an integer range defaults to 1. With `--sample`, a range without a step covers every value from
    /// min to max. `action.NAME=TICKS` varies the tick of an action `--act` adds. Repeatable. Every combination runs
    /// unless `--zip` or `--sample` is given.
    #[arg(long, value_name = "ID=LEVELS", requires = "explore")]
    pub vary: Vec<String>,

    /// Pair the levels of every `--vary` by position instead of running every combination.
    #[arg(long, requires = "vary")]
    pub zip: bool,

    /// Draw N configs from the `--vary` levels and ranges, spread as a Latin hypercube (`lhs:N`) or drawn
    /// uniformly (`random:N`).
    #[arg(
        long,
        value_name = "lhs:N|random:N",
        value_parser = parse_sample,
        requires = "vary",
        conflicts_with = "zip"
    )]
    pub sample: Option<DesignKind>,

    /// Seed for the `--sample` draws. Defaults to a seed derived from `--seed`.
    #[arg(long, value_name = "N", requires = "sample")]
    pub design_seed: Option<u64>,

    /// Run one config per row of a CSV file. The header names parameter ids, or `action.NAME` for the tick of an
    /// action `--act` adds.
    #[arg(long, value_name = "FILE", requires = "explore", conflicts_with_all = ["vary", "zip", "sample"])]
    pub design: Option<PathBuf>,

    /// Give every run its own seed. By default, replicate `r` of every config shares one seed.
    #[arg(long, requires = "explore")]
    pub independent_seeds: bool,

    /// Keep a series row every N ticks, a multiple of `--stats-every`. 0 keeps no series. Defaults to
    /// `--stats-every`.
    #[arg(long, value_name = "N", requires = "explore")]
    pub series_every: Option<u64>,

    /// End a run at the first sample where a condition holds, e.g. `'Infected <= 0'`. The comparator is one of
    /// `<`, `<=`, `>`, `>=`, `==` and `!=`.
    #[arg(long, value_name = "CONDITION", requires = "explore")]
    pub stop: Option<String>,

    /// Add a reducer over a stat column, e.g. `Infected:max`. Kinds are final, min, max, mean, argmax, argmin,
    /// `first<=10` for the first tick where a comparison holds, and `mean@200..600` for the mean from tick 200 to
    /// tick 600. Repeatable.
    #[arg(long, value_name = "COLUMN:KIND", requires = "explore")]
    pub reduce: Vec<String>,

    /// Drop the final, min, max and mean reducers every stat column gets by default.
    #[arg(long, requires = "explore")]
    pub no_default_reducers: bool,

    /// End a run after this many seconds of wall-clock time and record it as `timed_out`. Beside other GPU runs, a
    /// run's clock counts its share of the device. With `--resume`, it will run again.
    #[arg(long, value_name = "SECONDS", value_parser = parse_timeout, requires = "explore")]
    pub timeout: Option<Duration>,

    /// Number of concurrent runs, or `auto` [default: auto].
    #[arg(long, value_name = "N|auto", requires = "explore")]
    pub concurrent: Option<Concurrency>,

    /// Combined memory limit in bytes for concurrent runs.
    #[arg(long, value_name = "BYTES", requires = "explore")]
    pub memory: Option<u64>,

    /// Combined GPU memory limit in bytes for concurrent GPU runs. Defaults to the device's largest buffer size.
    #[arg(long, value_name = "BYTES", requires = "explore")]
    pub gpu_memory: Option<u64>,

    /// Run only the runs whose id leaves remainder I when divided by N. Join the shard directories with `--merge`.
    #[arg(long, value_name = "I/N", requires = "explore")]
    pub shard: Option<Shard>,

    /// Resume the sweep in the `--out` directory and run only the missing and timed-out runs, including new
    /// replicates from a higher `--reps`.
    #[arg(long, requires = "out")]
    pub resume: bool,

    /// Also rerun failed runs when resuming.
    #[arg(long, requires = "resume")]
    pub retry_failed: bool,

    /// Merge the directories of a sweep's shards into the `--out` directory. Takes no model.
    #[arg(
        long,
        value_name = "DIR",
        num_args = 1..,
        requires = "out",
        conflicts_with_all = [
            "model", "list", "info", "params", "set", "act", "steps", "warmup", "reps", "seed", "stats_every",
            "spec", "dry_run", "vary", "zip", "sample", "design_seed", "design", "independent_seeds",
            "series_every", "stop", "reduce", "no_default_reducers", "timeout", "concurrent", "memory", "gpu_memory",
            "shard", "resume", "retry_failed",
        ]
    )]
    pub merge: Vec<PathBuf>,

    /// Print the sweep's plan and write nothing. The first config that builds without a fault and the last config
    /// are built as a check.
    #[arg(long)]
    pub dry_run: bool,
}

impl ExploreArgs {
    /// Returns whether the flags ask for a sweep.
    pub fn is_sweep(&self) -> bool {
        self.out.is_some() || self.spec.is_some() || self.dry_run
    }
}

/// Runs the sweep the flags or `spec` describe over `entry`, and returns the exit status.
///
/// The status is success when every run is `ok`, and [`SOME_RUNS_NOT_OK`] when the sweep ran to its end with some
/// run in another status.
///
/// # Errors
///
/// Returns an error when the flags or the spec cannot make a sweep, the sweep cannot be planned, or its results
/// cannot be written.
pub fn run(args: &Args, entry: &ModelEntry, gpu: Option<&GpuContext>, spec: Option<LoadedSpec>) -> Result<ExitCode> {
    let (sweep, options) = sweep_and_options(args, entry, spec)?;
    if cfg!(debug_assertions) && !args.explore.dry_run {
        eprintln!("!!! warning: debug build. Runs will step slowly and timings will be unreliable. Use --release !!!");
    }
    let mut reporter = Reporter::new(entry.name(), args, options.spec_source.path.is_some());
    let report = match &args.explore.out {
        _ if args.explore.dry_run => {
            plan_spec(entry, gpu, &sweep, args.explore.out.as_deref(), &options, &mut reporter)?
        }
        Some(output_dir) => {
            let output = SweepOutput::Directory(output_dir.clone());
            run_spec(entry, gpu, &sweep, output, &options, &mut reporter)?.report
        }
        None => bail!("a sweep needs an output directory unless it is a dry run"),
    };
    exit_status(report.end, &report.counts)
}

/// Returns the sweep the flags or `spec` describe, and its options.
///
/// A spec's `[execution]` table goes in first, and each of `--concurrent`, `--memory` and `--gpu-memory` given on
/// the command line then replaces its setting.
///
/// # Errors
///
/// Returns an error when the flags cannot make a sweep.
fn sweep_and_options(args: &Args, entry: &ModelEntry, spec: Option<LoadedSpec>) -> Result<(SweepSpec, SweepOptions)> {
    let mut options = SweepOptions::new(provenance());
    let sweep = if let Some(loaded) = spec {
        options.apply_execution(&loaded.execution);
        options.spec_source = loaded.spec_source;
        loaded.spec
    } else {
        let sweep = spec_from_flags(args, entry.id())?;
        options.spec_source = flags_source(args, &sweep);
        sweep
    };
    if let Some(concurrency) = args.explore.concurrent {
        options.concurrency = concurrency;
    }
    if let Some(memory) = args.explore.memory {
        options.memory_budget = Some(memory);
    }
    if let Some(gpu_memory) = args.explore.gpu_memory {
        options.gpu_memory = Some(gpu_memory);
    }
    options.shard = args.explore.shard.unwrap_or_default();
    options.resume = args.explore.resume;
    options.retry_failed = args.explore.retry_failed;
    Ok((sweep, options))
}

/// Merges the shard directories `--merge` names into the `--out` directory, and returns the exit status.
///
/// The status is success when the merged directory holds every run of the plan and each is `ok`, and
/// [`SOME_RUNS_NOT_OK`] otherwise.
///
/// # Errors
///
/// Returns an error when the directories cannot be merged.
pub fn merge_shards(args: &Args) -> Result<ExitCode> {
    let output_dir = args.explore.out.as_deref().context("--merge requires --out")?;
    let report = merge(&args.explore.merge, output_dir, &mut WarningPrinter { json: args.json })?;
    if args.json {
        json_report::emit(&merge_json(&report, args.explore.merge.len()));
    } else {
        eprintln!("{}", merge_text(&report, args.explore.merge.len()));
    }
    let counts = &report.counts;
    if report.missing == 0 && counts.ok == counts.rows {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(SOME_RUNS_NOT_OK))
    }
}

/// Returns the exit status of a sweep or search that ended as `end` with `counts` rows written.
///
/// # Errors
///
/// Returns an error for a sweep that was aborted or lost its GPU device.
fn exit_status(end: SweepEnd, counts: &ResultCounts) -> Result<ExitCode> {
    match end {
        SweepEnd::Planned => Ok(ExitCode::SUCCESS),
        SweepEnd::Complete if counts.ok == counts.rows => Ok(ExitCode::SUCCESS),
        SweepEnd::Complete => Ok(ExitCode::from(SOME_RUNS_NOT_OK)),
        SweepEnd::Aborted => bail!("the sweep was aborted after {} runs", counts.rows),
        SweepEnd::DeviceLost => bail!(
            "the GPU device was lost after {} runs. Use --resume to run the rest",
            counts.rows
        ),
    }
}

/// Returns the sweep the flags describe, one block over every `--vary` or over the `--design` table.
///
/// # Errors
///
/// Returns an error for a `--set`, `--act`, `--vary`, `--stop` or `--reduce` that cannot be read, a `--vary` over the
/// tick of an action no `--act` adds, or a `--design` table that cannot be read.
pub fn spec_from_flags(args: &Args, model: &str) -> Result<SweepSpec> {
    let reducers = args
        .explore
        .reduce
        .iter()
        .map(|raw| raw.parse::<ReducerSpec>().with_context(|| format!("--reduce {raw}")))
        .collect::<Result<Vec<_>>>()?;
    let factors = args
        .explore
        .vary
        .iter()
        .map(|raw| parse_vary(raw))
        .collect::<Result<Vec<_>>>()?;
    let stop = args
        .explore
        .stop
        .as_deref()
        .map(|raw| StopSpec::parse(raw, 0).with_context(|| format!("--stop {raw}")))
        .transpose()?;
    let mut spec = SweepSpec::new(model);
    spec.fixed = parse_overrides(&args.set)?;
    spec.run = RunSettings {
        steps: args.steps,
        warmup: args.warmup,
        replicates: args.reps,
        stop,
        timeout: args.explore.timeout,
    };
    spec.measure = MeasureSettings {
        stats_every: args.stats_every,
        series_every: args.explore.series_every.unwrap_or(args.stats_every),
        default_reducers: !args.explore.no_default_reducers,
        reducers,
    };
    spec.seeds = SeedSettings {
        root: args.seed.unwrap_or(0),
        scheme: if args.explore.independent_seeds {
            SeedScheme::Independent
        } else {
            SeedScheme::Common
        },
    };
    spec.actions = fixed_actions(&args.act)?;
    for factor in &factors {
        if let FactorTarget::Action(name) = &factor.target
            && !spec.actions.iter().any(|action| action.name == *name)
        {
            bail!("--vary {ACTION_COLUMN_PREFIX}{name}: no action named '{name}' in any --act");
        }
    }
    if let Some(path) = &args.explore.design {
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot read design table '{}'", path.display()))?;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Table { text },
            factors: Vec::new(),
            design_seed: None,
        }];
    } else if !factors.is_empty() {
        let design = match (&args.explore.sample, args.explore.zip) {
            (Some(sample), _) => sample.clone(),
            (None, true) => DesignKind::Zip,
            (None, false) => DesignKind::Factorial,
        };
        spec.blocks = vec![BlockSpec {
            design,
            factors,
            design_seed: args.explore.design_seed,
        }];
    }
    Ok(spec)
}

/// Returns the record of a sweep built from flags, listing the `--design` table it read.
fn flags_source(args: &Args, spec: &SweepSpec) -> SpecSource {
    let tables = args
        .explore
        .design
        .iter()
        .zip(&spec.blocks)
        .filter_map(|(path, block)| match &block.design {
            DesignKind::Table { text } => Some(DesignTableFile {
                path: path.clone(),
                fnv1a64: fnv1a64(text.as_bytes()),
            }),
            _ => None,
        })
        .collect();
    SpecSource {
        tables,
        ..SpecSource::default()
    }
}

/// Reads `--vary ID=LEVELS` into a factor, with `LEVELS` in the text [`LevelSpec::parse`] reads.
///
/// An id of the form `action.NAME` makes a factor over the tick of the action named `NAME`. The model checks the
/// levels when the sweep is planned.
///
/// # Errors
///
/// Returns an error for text with no `=`, or levels [`LevelSpec::parse`] refuses.
pub fn parse_vary(raw: &str) -> Result<FactorSpec> {
    let (target, levels) = raw
        .split_once('=')
        .with_context(|| format!("invalid --vary '{raw}', expected ID=LEVELS"))?;
    let levels = LevelSpec::parse(levels).with_context(|| format!("--vary {target}"))?;
    Ok(match target.strip_prefix(ACTION_COLUMN_PREFIX) {
        Some(name) => FactorSpec::action(name, levels),
        None => FactorSpec::param(target, levels),
    })
}

/// Returns the actions `--act` adds to every run of a sweep.
///
/// An action is named by its id, or by the first of `ID_2`, `ID_3` and so on that no earlier action has taken. The
/// second `--act` of an id is then `ID_2` and the third `ID_3`, unless an earlier entry took the name. The model
/// checks the ids when the sweep is planned.
///
/// # Errors
///
/// Returns [`ScheduleError`] for an entry that does not read as `ID@TICK`.
pub fn fixed_actions(raw: &[String]) -> Result<Vec<ActionSpec>, ScheduleError> {
    let mut actions: Vec<ActionSpec> = Vec::with_capacity(raw.len());
    for entry in raw {
        let (id, tick) = entry
            .split_once('@')
            .ok_or_else(|| ScheduleError::BadEntry { raw: entry.clone() })?;
        let tick = tick.parse().map_err(|source| ScheduleError::BadTick {
            raw: entry.clone(),
            source,
        })?;
        let mut action = ActionSpec::new(id, tick);
        let name_taken = |name: &str| actions.iter().any(|earlier| earlier.name == name);
        if name_taken(id) {
            action.name = (2_u64..)
                .map(|count| format!("{id}_{count}"))
                .find(|name| !name_taken(name))
                .unwrap_or_default();
        }
        actions.push(action);
    }
    Ok(actions)
}

/// Reads `--sample` as `lhs:N` or `random:N`.
fn parse_sample(raw: &str) -> Result<DesignKind, String> {
    let refuse = || format!("expected lhs:N or random:N with N at least 1, got '{raw}'");
    let (design, samples) = raw.split_once(':').ok_or_else(refuse)?;
    let samples = samples
        .parse::<usize>()
        .ok()
        .filter(|&samples| samples > 0)
        .ok_or_else(refuse)?;
    match design {
        "lhs" => Ok(DesignKind::LatinHypercube { samples }),
        "random" => Ok(DesignKind::Random { samples }),
        _ => Err(refuse()),
    }
}

/// Reads `--timeout` as a number of seconds from 0.
fn parse_timeout(raw: &str) -> Result<Duration, String> {
    raw.parse::<f64>()
        .ok()
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .ok_or_else(|| format!("expected a non-negative number of seconds, got '{raw}'"))
}

/// Returns the build of this binary and its command line.
fn provenance() -> Provenance {
    let arguments = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    Provenance::new(henad_core::build_info!(), arguments)
}

/// Progress of a sweep as JSON lines on stdout, or as text on stderr.
///
/// On a terminal the text is one line that rewrites itself. Elsewhere a line is logged every
/// [`LOGGED_PROGRESS_INTERVAL`].
pub struct Reporter {
    /// Name of the model, as a person reads it.
    model_name: String,
    json: bool,
    dry_run: bool,
    resume: bool,
    /// Whether the sweep came from a `--spec` file rather than flags.
    from_spec: bool,
    terminal: bool,
    /// Runs the output directory holds already. The sweep skips them.
    skipped: u64,
    /// Characters of the progress line on the terminal, 0 when none is showing.
    shown_width: usize,
    last_logged: Option<Instant>,
    /// Outline of a search, `None` for a sweep.
    search: Option<SearchOutline>,
    /// Standing of a search after its latest batch.
    search_standing: Option<SearchStanding>,
}

/// Best candidate or filled cells of a search after a batch, and the evaluations told.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SearchStanding {
    evaluations: u64,
    /// Best candidate and its objective, over its replicates.
    best: Option<(u64, f64, u64)>,
    /// Generations of a genetic algorithm finished so far.
    generation_count: u64,
    filled_cells: u64,
}

impl Reporter {
    pub fn new(model_name: &str, args: &Args, from_spec: bool) -> Self {
        Self {
            model_name: model_name.to_owned(),
            json: args.json,
            dry_run: args.explore.dry_run,
            resume: args.explore.resume,
            from_spec,
            terminal: io::stderr().is_terminal(),
            skipped: 0,
            shown_width: 0,
            last_logged: None,
            search: None,
            search_standing: None,
        }
    }

    /// Clears the progress line from the terminal.
    fn clear_line(&mut self) {
        if self.shown_width > 0 {
            eprint!("\r{:width$}\r", "", width = self.shown_width);
            self.shown_width = 0;
        }
    }

    fn planned(&mut self, outline: &SweepOutline) {
        self.skipped = outline.skipped;
        self.search.clone_from(&outline.search);
        if self.json {
            json_report::emit(&plan_json(outline));
        } else if self.dry_run {
            println!("{}", plan_text(&self.model_name, outline, self.resume));
        } else {
            eprintln!("{}", plan_text(&self.model_name, outline, self.resume));
        }
        if outline.series_rows > SERIES_ROWS_WARNING {
            let name = if self.from_spec {
                "series_every"
            } else {
                "--series-every"
            };
            eprintln!(
                "warning: series.csv will hold about {} rows. \
                 Increase {name} to write fewer, or set it to 0 to write none.",
                outline.series_rows
            );
        }
    }

    fn committed(&mut self, outcome: &RunOutcome) {
        if self.json {
            json_report::emit(&run_json(outcome));
        } else if outcome.status != RunStatus::Ok {
            self.clear_line();
            let run = &outcome.run;
            let note = outcome.note.as_deref().unwrap_or("");
            eprintln!(
                "  run {} (config {}, rep {}, seed {}): {}: {note}",
                run.run_id,
                run.config_id,
                run.rep,
                run.seed,
                outcome.status.as_str()
            );
        }
    }

    fn progressed(&mut self, update: &ProgressUpdate) {
        if self.json {
            json_report::emit(&progress_json(update, self.skipped));
            return;
        }
        let mut line = progress_text(update, self.skipped);
        if let (Some(search), Some(standing)) = (&self.search, &self.search_standing) {
            line.push_str(&format!(", {}", standing_text(search, standing)));
        }
        if self.terminal {
            let width = self.shown_width.max(line.len());
            eprint!("\r{line:width$}");
            self.shown_width = line.len();
        } else if self
            .last_logged
            .is_none_or(|logged| logged.elapsed() >= LOGGED_PROGRESS_INTERVAL)
        {
            eprintln!("{line}");
            self.last_logged = Some(Instant::now());
        }
    }

    fn search_batch_told(&mut self, update: &SearchUpdate) {
        let finished = self.search_standing.map_or(0, |standing| standing.generation_count);
        let standing = SearchStanding {
            evaluations: update.evaluations,
            best: update
                .best
                .as_ref()
                .map(|entry| (entry.candidate_id, entry.objective, entry.replicate_count)),
            generation_count: finished + update.generations.len() as u64,
            filled_cells: update.filled_cells,
        };
        self.search_standing = Some(standing);
        if self.json
            && let Some(search) = &self.search
        {
            json_report::emit(&search_batch_json(search, update, &standing));
        }
    }

    fn ended(&mut self, report: &SweepReport) {
        if self.json {
            json_report::emit(&end_json(report, self.search_standing.as_ref()));
            return;
        }
        self.clear_line();
        let counts = &report.counts;
        let skipped = report.outline.skipped;
        let written = counts.rows.saturating_sub(skipped);
        let elapsed = format_seconds(report.elapsed.as_secs_f64());
        let dir = report
            .output_dir
            .as_deref()
            .map_or_else(String::new, |dir| dir.display().to_string());
        let tally = format!(
            "{} ok, {} non-finite, {} failed",
            counts.ok, counts.non_finite, counts.failed
        );
        match report.end {
            SweepEnd::Planned => eprintln!("dry run, nothing written"),
            SweepEnd::Complete if skipped > 0 => {
                eprintln!("wrote {written} runs to {dir} in {elapsed} and skipped {skipped}: {tally} in total");
            }
            SweepEnd::Complete => eprintln!("wrote {written} runs to {dir} in {elapsed}: {tally}"),
            SweepEnd::Aborted => eprintln!("aborted after {written} runs, written to {dir}"),
            SweepEnd::DeviceLost => eprintln!("GPU device lost after {written} runs, written to {dir}"),
        }
        if let (Some(search), Some(standing), false) =
            (&self.search, &self.search_standing, report.end == SweepEnd::Planned)
        {
            eprintln!("{}", standing_text(search, standing));
        }
    }
}

impl Progress for Reporter {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        match event {
            ProgressEvent::Planned(outline) => self.planned(outline),
            ProgressEvent::Warned(warning) => {
                self.clear_line();
                eprintln!("warning: {warning}");
                if self.json {
                    json_report::emit(&warning_json(warning));
                }
            }
            ProgressEvent::RunCommitted(outcome) => self.committed(outcome),
            ProgressEvent::Progressed(update) => self.progressed(update),
            ProgressEvent::SearchBatchTold(update) => self.search_batch_told(update),
            ProgressEvent::Ended(report) => self.ended(report),
        }
    }
}

/// Progress that prints a merge's warnings and nothing else, each also as a JSON line under `--json`.
struct WarningPrinter {
    json: bool,
}

impl Progress for WarningPrinter {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        if let ProgressEvent::Warned(warning) = event {
            eprintln!("warning: {warning}");
            if self.json {
                json_report::emit(&warning_json(warning));
            }
        }
    }
}

/// Returns the `explore_warning` line of `warning`.
///
/// `warning` names its kind, `message` holds the text the warning prints, and a `build_changed` warning adds its
/// `role`, the `recorded` and `current` builds as the manifest records them, and `between_shards`, true when a merge
/// found `current` in another shard.
fn warning_json(warning: &SweepWarning) -> Value {
    let mut line = json!({
        "kind": "explore_warning",
        "message": warning.to_string(),
    });
    let kind = match warning {
        SweepWarning::Plan(_) => "plan",
        SweepWarning::MissingRuns { .. } => "missing_runs",
        SweepWarning::BuildChanged {
            role,
            recorded,
            current,
            between_shards,
        } => {
            line["role"] = json!(role.as_str());
            line["between_shards"] = json!(between_shards);
            line["recorded"] = serde_json::to_value(recorded.as_ref()).unwrap_or_default();
            line["current"] = serde_json::to_value(current.as_ref()).unwrap_or_default();
            "build_changed"
        }
    };
    line["warning"] = json!(kind);
    line
}

/// Returns the plan of a sweep over the model named `model_name`, as the lines a person reads.
///
/// A `resume` adds a line counting the runs skipped and the runs to run.
fn plan_text(model_name: &str, outline: &SweepOutline, resume: bool) -> String {
    let mut text = match &outline.search {
        Some(search) => format!(
            "search of {model_name} ({}): {}, {} x {} = {}\n",
            outline.model,
            search.algorithm,
            plural(search.max_evaluations, "evaluation"),
            plural(outline.replicates, "replicate"),
            plural(outline.runs, "run")
        ),
        None => format!(
            "sweep of {model_name} ({}): {} x {} = {}\n",
            outline.model,
            plural(outline.configs.unwrap_or_default(), "config"),
            plural(outline.replicates, "replicate"),
            plural(outline.runs, "run")
        ),
    };
    // A line with no label continues the list of the line above.
    let mut line = |label: &str, value: String| {
        let label = if label.is_empty() {
            String::new()
        } else {
            format!("{label}:")
        };
        text.push_str(&format!("  {label:<14}{value}\n"));
    };
    for (index, block) in outline.blocks.iter().enumerate() {
        line(&format!("block {index}"), block_text(block));
    }
    if let Some(search) = &outline.search {
        line("batch size", plural(search.batch_size as u64, "candidate"));
        if let Some(objective) = &search.objective {
            line("objective", objective_text(objective));
        }
        for (index, axis) in search.pattern_axes.iter().flatten().enumerate() {
            let label = if index == 0 { "axes" } else { "" };
            line(label, axis_text(axis));
        }
        for (index, factor) in search.space.iter().enumerate() {
            let label = if index == 0 { "search space" } else { "" };
            line(label, factor_text(factor));
        }
        line("search seed", search.search_seed.to_string());
    }
    if outline.shard.count() > 1 {
        let runs = outline.shard.run_count(outline.runs);
        line("shard", format!("{}, {}", outline.shard, plural(runs, "run")));
    }
    if resume {
        line(
            "resume",
            format!("{} skipped, {} to run", plural(outline.skipped, "run"), outline.pending),
        );
    }
    line("layout", layout_text(&outline.layout));
    line(
        "memory",
        format!(
            "{} projected for concurrent runs",
            format_bytes(outline.projected_bytes)
        ),
    );
    line("series rows", outline.series_rows.to_string());
    text.pop();
    text
}

/// Returns a block's design, config count and design seed, as in `lhs, 12 configs, design seed 42`.
fn block_text(block: &PlannedBlock) -> String {
    let configs = plural(block.configs.end - block.configs.start, "config");
    match block.design_seed {
        Some(seed) => format!("{}, {configs}, design seed {seed}", block.design.as_str()),
        None => format!("{}, {configs}", block.design.as_str()),
    }
}

/// Returns an objective as in `minimize the median of Infected:max`.
fn objective_text(objective: &Objective) -> String {
    format!(
        "{} the {} of {}",
        objective.goal.as_str(),
        objective.aggregate.as_str(),
        objective.column
    )
}

/// Returns an axis of a Pattern Space Exploration as in `Infected:max from 0 to 4096, 32 cells` or
/// `Infected:max, automatic range, 32 cells`.
fn axis_text(axis: &PatternAxis) -> String {
    let cells = plural(u64::from(axis.cells), "cell");
    match axis.range() {
        Some((min, max)) => format!("{} from {min} to {max}, {cells}", axis.column),
        None => format!("{}, automatic range, {cells}", axis.column),
    }
}

/// Returns a factor of a search space as in `infection_rate from 0.05 to 0.9` or `network in Random, Geometric`.
fn factor_text(factor: &FactorSpec) -> String {
    let target = match &factor.target {
        FactorTarget::Param(id) => id.clone(),
        FactorTarget::Action(name) => format!("{ACTION_COLUMN_PREFIX}{name}"),
    };
    match &factor.levels {
        LevelSpec::Values(values) => format!("{target} in {}", values.join(", ")),
        LevelSpec::Range { min, max, step: None } => format!("{target} from {min} to {max}"),
        LevelSpec::Range {
            min,
            max,
            step: Some(step),
        } => format!("{target} from {min} to {max} in steps of {step}"),
        LevelSpec::All => format!("{target}, every value"),
    }
}

/// Returns the standing of `search` after a batch, as in
/// `40/480 evaluations, best candidate 17: Infected:max 102, over 8 replicates` or
/// `96/1200 evaluations, 45 cells filled`.
fn standing_text(search: &SearchOutline, standing: &SearchStanding) -> String {
    let evaluations = format!("{}/{} evaluations", standing.evaluations, search.max_evaluations);
    match (&search.objective, standing.best) {
        (Some(objective), Some((candidate_id, value, replicates))) if value.is_finite() => format!(
            "{evaluations}, best candidate {candidate_id}: {} {value}, over {}",
            objective.column,
            plural(replicates, "replicate")
        ),
        (Some(_), _) => format!("{evaluations}, no candidate with a finite objective"),
        (None, _) => format!("{evaluations}, {} filled", plural(standing.filled_cells, "cell")),
    }
}

fn layout_text(layout: &ExecutionLayout) -> String {
    if layout.gpu_tracks > 0 {
        format!("{} at a time", plural(layout.gpu_tracks as u64, "GPU run"))
    } else {
        let threads = plural(layout.threads_per_lane as u64, "thread");
        match layout.cpu_lanes {
            1 => format!("1 lane of {threads}"),
            lanes => format!("{lanes} lanes of {threads} each"),
        }
    }
}

fn progress_text(update: &ProgressUpdate, skipped: u64) -> String {
    let left = update.remaining_s.map_or_else(String::new, |remaining| {
        format!(", about {} left", format_seconds(remaining))
    });
    let skipped = if skipped > 0 {
        format!(", {skipped} skipped")
    } else {
        String::new()
    };
    format!(
        "  {}/{} runs{skipped}, {} failed, {} elapsed{left}",
        update.done,
        update.total,
        update.failed,
        format_seconds(update.elapsed_s)
    )
}

/// Returns the end of a merge of `inputs` shard directories, as the line a person reads.
fn merge_text(report: &MergeReport, inputs: usize) -> String {
    let counts = &report.counts;
    let missing = if report.missing > 0 {
        format!(", {} missing", report.missing)
    } else {
        String::new()
    };
    format!(
        "merged {} into {}: {}, {} ok, {} non-finite, {} failed{missing}",
        plural(inputs as u64, "shard"),
        report.output_dir.display(),
        plural(counts.rows, "run"),
        counts.ok,
        counts.non_finite,
        counts.failed
    )
}

/// Returns `number` followed by `noun`, with an `s` unless `number` is 1.
fn plural(number: u64, noun: &str) -> String {
    if number == 1 {
        format!("{number} {noun}")
    } else {
        format!("{number} {noun}s")
    }
}

/// Returns `bytes` in the largest binary unit that keeps the value at 1 or more.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Returns `seconds` as `4.2s`, `42s`, `3m 07s` or `2h 05m`.
fn format_seconds(seconds: f64) -> String {
    if seconds < 10.0 {
        return format!("{seconds:.1}s");
    }
    let whole = seconds.round() as u64;
    let (hours, minutes, rest) = (whole / 3600, whole / 60 % 60, whole % 60);
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {rest:02}s")
    } else {
        format!("{rest}s")
    }
}

fn end_name(end: SweepEnd) -> &'static str {
    match end {
        SweepEnd::Planned => "planned",
        SweepEnd::Complete => "complete",
        SweepEnd::Aborted => "aborted",
        SweepEnd::DeviceLost => "device_lost",
    }
}

fn plan_json(outline: &SweepOutline) -> Value {
    let blocks: Vec<Value> = outline
        .blocks
        .iter()
        .map(|block| {
            json!({
                "design": block.design.as_str(),
                "configs": block.configs.end - block.configs.start,
                "design_seed": block.design_seed,
            })
        })
        .collect();
    json!({
        "kind": "explore_plan",
        "model": outline.model,
        "backend": backend_name(outline.backend),
        "configs": outline.configs,
        "replicates": outline.replicates,
        "runs": outline.runs,
        "blocks": blocks,
        "shard": { "index": outline.shard.index(), "count": outline.shard.count() },
        "skipped": outline.skipped,
        "pending": outline.pending,
        "cpu_lanes": outline.layout.cpu_lanes,
        "threads_per_lane": outline.layout.threads_per_lane,
        "gpu_tracks": outline.layout.gpu_tracks,
        "projected_bytes": outline.projected_bytes,
        "series_rows": outline.series_rows,
        "dry_run": outline.dry_run,
        "search": outline.search.as_ref().map(search_json),
    })
}

fn search_json(search: &SearchOutline) -> Value {
    json!({
        "algorithm": search.algorithm,
        "max_evaluations": search.max_evaluations,
        "batch_size": search.batch_size,
        "objective": search.objective.as_ref().map(|objective| json!({
            "column": objective.column,
            "goal": objective.goal.as_str(),
            "aggregate": objective.aggregate.as_str(),
        })),
        "watched_columns": search.watched_columns,
        "axes": search
            .pattern_axes
            .as_ref()
            .map(|axes| axes.iter().map(axis_text).collect::<Vec<_>>()),
        "space": search.space.iter().map(factor_text).collect::<Vec<_>>(),
        "search_seed": search.search_seed,
    })
}

/// Returns one batch of `search`, with the best candidate of a scored search, the number of generations a genetic
/// algorithm has finished, or the cells a Pattern Space Exploration fills.
fn search_batch_json(search: &SearchOutline, update: &SearchUpdate, standing: &SearchStanding) -> Value {
    let mut batch = json!({
        "kind": "explore_search_batch",
        "batch": update.batch,
        "evaluations": update.evaluations,
        "runs": update.runs,
    });
    let Some(fields) = batch.as_object_mut() else {
        return batch;
    };
    if search.objective.is_some() {
        let best = update.best.as_ref();
        fields.insert(
            "best_candidate_id".to_owned(),
            json!(best.map(|entry| entry.candidate_id)),
        );
        fields.insert(
            "best_objective".to_owned(),
            json!(
                best.map(|entry| entry.objective)
                    .filter(|objective| objective.is_finite())
            ),
        );
        fields.insert(
            "best_replicates".to_owned(),
            json!(best.map(|entry| entry.replicate_count)),
        );
    } else {
        fields.insert("filled_cells".to_owned(), json!(update.filled_cells));
    }
    if search.algorithm == "genetic" {
        fields.insert("generation_count".to_owned(), json!(standing.generation_count));
    }
    batch
}

fn run_json(outcome: &RunOutcome) -> Value {
    let run = &outcome.run;
    json!({
        "kind": "explore_run",
        "run_id": run.run_id,
        "config_id": run.config_id,
        "rep": run.rep,
        "seed": run.seed,
        "status": outcome.status.as_str(),
        "stop_reason": outcome.stop_reason.as_str(),
        "ticks": outcome.ticks,
        "wall_ms": outcome.wall_ms,
    })
}

fn progress_json(update: &ProgressUpdate, skipped: u64) -> Value {
    json!({
        "kind": "explore_progress",
        "done": update.done,
        "total": update.total,
        "skipped": skipped,
        "failed": update.failed,
        "elapsed_s": update.elapsed_s,
        "remaining_s": update.remaining_s,
    })
}

fn end_json(report: &SweepReport, standing: Option<&SearchStanding>) -> Value {
    let counts = &report.counts;
    let mut end = json!({
        "kind": "explore_end",
        "end": end_name(report.end),
        "rows": counts.rows,
        "skipped": report.outline.skipped,
        "ok": counts.ok,
        "non_finite": counts.non_finite,
        "failed": counts.failed,
        "elapsed_s": report.elapsed.as_secs_f64(),
        "output_dir": report.output_dir.as_ref().map(|dir| dir.display().to_string()),
    });
    if let (Some(search), Some(fields)) = (&report.outline.search, end.as_object_mut()) {
        let best = standing.and_then(|standing| standing.best);
        fields.insert(
            "evaluations".to_owned(),
            json!(standing.map_or(0, |standing| standing.evaluations)),
        );
        if search.objective.is_some() {
            fields.insert(
                "best_candidate_id".to_owned(),
                json!(best.map(|(candidate_id, _, _)| candidate_id)),
            );
            fields.insert(
                "best_objective".to_owned(),
                json!(
                    best.map(|(_, objective, _)| objective)
                        .filter(|objective| objective.is_finite())
                ),
            );
        } else {
            fields.insert(
                "filled_cells".to_owned(),
                json!(standing.map_or(0, |standing| standing.filled_cells)),
            );
        }
    }
    end
}

fn merge_json(report: &MergeReport, inputs: usize) -> Value {
    let counts = &report.counts;
    json!({
        "kind": "explore_merge",
        "inputs": inputs,
        "rows": counts.rows,
        "ok": counts.ok,
        "non_finite": counts.non_finite,
        "failed": counts.failed,
        "missing": report.missing,
        "output_dir": report.output_dir.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::time::Duration;

    use clap::Parser as _;
    use clap::error::ErrorKind;
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorTarget, LevelSpec};
    use henad_core::explore::plan::Shard;
    use henad_core::explore::seed::SeedScheme;
    use henad_core::explore::spec::ActionSpec;
    use henad_core::explore::stop::StopSpec;
    use henad_explore::exec::Concurrency;
    use henad_explore::output::manifest::ResultCounts;
    use henad_explore::sweep::{SweepEnd, SweepOptions, plan_spec};
    use henad_models::example_models;

    use super::{
        LoadedSpec, PatternAxis, SOME_RUNS_NOT_OK, axis_text, exit_status, fixed_actions, format_bytes, format_seconds,
        parse_vary, plan_text, provenance, spec_from_flags, sweep_and_options,
    };
    use crate::{Args, Mode};

    fn levels(raw: &str) -> LevelSpec {
        parse_vary(raw).expect("reads").levels
    }

    fn values(raw: &[&str]) -> LevelSpec {
        LevelSpec::Values(raw.iter().map(|&value| value.to_owned()).collect())
    }

    #[test]
    fn vary_parses_lists_ranges_and_all() {
        let factor = parse_vary("infection_rate=0.1:0.5:0.1").expect("reads");
        assert_eq!(factor.target, FactorTarget::Param("infection_rate".to_owned()));
        assert_eq!(
            factor.levels,
            LevelSpec::Range {
                min: 0.1,
                max: 0.5,
                step: Some(0.1)
            }
        );
        assert_eq!(
            levels("grid_width=16:64"),
            LevelSpec::Range {
                min: 16.0,
                max: 64.0,
                step: None
            }
        );
        assert_eq!(
            levels("offset=-1:1:0.5"),
            LevelSpec::Range {
                min: -1.0,
                max: 1.0,
                step: Some(0.5)
            }
        );
        assert_eq!(levels("network=Random,Geometric"), values(&["Random", "Geometric"]));
        assert_eq!(levels("recovery_rate=0.05"), values(&["0.05"]));
        assert_eq!(levels("directed=all"), LevelSpec::All);

        for bad in ["infection_rate", "infection_rate=0.1:x:0.1", "infection_rate=0:1:0.1:2"] {
            assert!(parse_vary(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn vary_parses_action_ticks() {
        let listed = parse_vary("action.second_wave=100,200,300").expect("reads");
        assert_eq!(listed.target, FactorTarget::Action("second_wave".to_owned()));
        assert_eq!(listed.levels, values(&["100", "200", "300"]));
        let ranged = parse_vary("action.seed_outbreak=0:400").expect("reads");
        assert_eq!(ranged.target, FactorTarget::Action("seed_outbreak".to_owned()));
        assert_eq!(
            ranged.levels,
            LevelSpec::Range {
                min: 0.0,
                max: 400.0,
                step: None
            }
        );
        let param = parse_vary("actions=1,2").expect("reads");
        assert_eq!(param.target, FactorTarget::Param("actions".to_owned()));
    }

    #[test]
    fn shard_parses_i_of_n() {
        let shard = |raw: &str| {
            Args::try_parse_from(["henad-cli", "sir", "--out", "d", "--shard", raw])
                .map(|args| args.explore.shard)
                .map_err(|error| error.kind())
        };
        assert_eq!(shard("1/4"), Ok(Some(Shard::new(1, 4).expect("1 is below 4"))));
        assert_eq!(shard("0/1"), Ok(Some(Shard::WHOLE)));
        for bad in ["4/4", "1/0", "1", "1/2/3", "a/b"] {
            assert_eq!(shard(bad), Err(ErrorKind::ValueValidation), "{bad}");
        }
        let whole = Args::parse_from(["henad-cli", "sir", "--out", "d"]);
        assert_eq!(whole.explore.shard, None);
    }

    #[test]
    fn merge_needs_output_and_no_model() {
        let merge =
            Args::try_parse_from(["henad-cli", "--merge", "shard-0", "shard-1", "--out", "merged"]).expect("a merge");
        assert_eq!(Mode::of(&merge), Mode::Merge);
        assert_eq!(
            merge.explore.merge,
            [PathBuf::from("shard-0"), PathBuf::from("shard-1")]
        );
        assert_eq!(merge.explore.out, Some(PathBuf::from("merged")));

        let refused: [(&[&str], ErrorKind); 8] = [
            (
                &["henad-cli", "--merge", "shard-0", "shard-1"],
                ErrorKind::MissingRequiredArgument,
            ),
            (
                &["henad-cli", "sir", "--merge", "shard-0", "--out", "merged"],
                ErrorKind::ArgumentConflict,
            ),
            (
                &["henad-cli", "--merge", "shard-0", "--out", "merged", "--spec", "s.toml"],
                ErrorKind::ArgumentConflict,
            ),
            (
                &["henad-cli", "--merge", "shard-0", "--out", "merged", "--resume"],
                ErrorKind::ArgumentConflict,
            ),
            (
                &["henad-cli", "--merge", "shard-0", "--out", "merged", "--retry-failed"],
                ErrorKind::ArgumentConflict,
            ),
            (
                &["henad-cli", "--merge", "shard-0", "--out", "merged", "--dry-run"],
                ErrorKind::ArgumentConflict,
            ),
            (
                &["henad-cli", "--merge", "shard-0", "--out", "merged", "--shard", "0/2"],
                ErrorKind::ArgumentConflict,
            ),
            (
                &["henad-cli", "--merge", "shard-0", "--out", "merged", "--params"],
                ErrorKind::ArgumentConflict,
            ),
        ];
        for (line, kind) in refused {
            assert_eq!(parse(line), Err(kind), "{line:?}");
        }
        assert!(
            parse(&["henad-cli", "--merge", "--out", "merged"]).is_err(),
            "no directory to merge"
        );
    }

    #[test]
    fn act_in_explore_mode_becomes_a_fixed_action() {
        let args = Args::parse_from([
            "henad-cli",
            "sir",
            "--out",
            "sweep",
            "--act",
            "seed_outbreak@40",
            "--act",
            "seed_outbreak@80",
            "--vary",
            "action.seed_outbreak_2=60,100",
        ]);
        assert_eq!(Mode::of(&args), Mode::Explore);
        let spec = spec_from_flags(&args, "sir").expect("a valid sweep");
        let action = |name: &str, tick| ActionSpec {
            id: "seed_outbreak".to_owned(),
            name: name.to_owned(),
            tick,
        };
        assert_eq!(
            spec.actions,
            [action("seed_outbreak", 40), action("seed_outbreak_2", 80)]
        );
        let sir = example_models().get("sir").cloned().expect("sir is registered");
        let plan = spec.plan(&sir.schema()).expect("sir declares seed_outbreak");
        let ticks: Vec<&[u64]> = plan
            .configs()
            .iter()
            .map(|config| config.action_ticks.as_slice())
            .collect();
        assert_eq!(ticks, [[40, 60], [40, 100]], "the second action's tick is the factor");

        for bad in ["seed_outbreak", "seed_outbreak@soon"] {
            assert!(fixed_actions(&[bad.to_owned()]).is_err(), "{bad}");
        }
        let benchmark = Args::parse_from(["henad-cli", "sir", "--act", "seed_outbreak@40"]);
        assert_eq!(
            Mode::of(&benchmark),
            Mode::Benchmark,
            "outside a sweep --act keeps its meaning"
        );
    }

    /// The regression. A repeated id was numbered by its count alone, so an explicit `ID_2` after it took the same
    /// name and the plan refused the sweep.
    #[test]
    fn a_repeated_action_takes_the_first_free_name() {
        let names = |raw: &[&str]| {
            let raw: Vec<String> = raw.iter().map(|&entry| entry.to_owned()).collect();
            fixed_actions(&raw)
                .expect("every entry reads")
                .into_iter()
                .map(|action| action.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&["seed@1", "seed@2", "seed@3"]), ["seed", "seed_2", "seed_3"]);
        assert_eq!(names(&["seed@1", "seed@2", "seed_2@3"]), ["seed", "seed_2", "seed_2_2"]);
        assert_eq!(names(&["seed@1", "seed_2@2", "seed@3"]), ["seed", "seed_2", "seed_3"]);
    }

    /// The regression. `--info` or `--list` with no model printed and exited, and the sweep asked for never ran.
    #[test]
    fn info_and_list_do_not_drop_a_sweep() {
        let mode = |line: &[&str]| {
            Args::try_parse_from(line)
                .map(|args| Mode::of(&args))
                .map_err(|error| error.kind())
        };
        assert_eq!(mode(&["henad-cli", "--info"]), Ok(Mode::InfoOnly));
        assert_eq!(
            mode(&["henad-cli", "--info", "--out", "d", "--vary", "infection_rate=0.1,0.2"]),
            Ok(Mode::Explore),
            "the sweep then asks for a model"
        );
        assert_eq!(mode(&["henad-cli", "--info", "--dry-run"]), Ok(Mode::Explore));
        assert_eq!(mode(&["henad-cli", "--info", "--spec", "s.toml"]), Ok(Mode::Explore));
        let listed: [&[&str]; 3] = [
            &["henad-cli", "--list", "--out", "d"],
            &["henad-cli", "--list", "--dry-run"],
            &["henad-cli", "--list", "--spec", "s.toml"],
        ];
        for line in listed {
            assert_eq!(parse(line), Err(ErrorKind::ArgumentConflict), "{line:?}");
        }
    }

    #[test]
    fn run_control_flags_reach_the_spec() {
        let args = Args::parse_from([
            "henad-cli",
            "sir",
            "--out",
            "sweep",
            "--vary",
            "infection_rate=0.1:0.9",
            "--vary",
            "recovery_rate=0.05,0.1",
            "--sample",
            "lhs:12",
            "--design-seed",
            "77",
            "--stop",
            "Infected <= 0",
            "--reduce",
            "Infected:argmax",
            "--reduce",
            "Infected:first<=10",
            "--reduce",
            "Recovered:mean@200..600",
            "--timeout",
            "2.5",
        ]);
        let spec = spec_from_flags(&args, "sir").expect("a valid sweep");
        assert_eq!(spec.blocks.len(), 1);
        assert_eq!(spec.blocks[0].design, DesignKind::LatinHypercube { samples: 12 });
        assert_eq!(spec.blocks[0].design_seed, Some(77));
        assert_eq!(spec.run.stop, Some(StopSpec::parse("Infected <= 0", 0).expect("reads")));
        assert_eq!(spec.run.timeout, Some(Duration::from_millis(2500)));
        let kinds: Vec<String> = spec
            .measure
            .reducers
            .iter()
            .map(|reducer| format!("{}:{}", reducer.column, reducer.kind))
            .collect();
        assert_eq!(
            kinds,
            ["Infected:argmax", "Infected:first<=10", "Recovered:mean@200..600"]
        );

        let random = Args::parse_from([
            "henad-cli",
            "sir",
            "--dry-run",
            "--vary",
            "infection_rate=0.1:0.9",
            "--sample",
            "random:5",
        ]);
        let spec = spec_from_flags(&random, "sir").expect("a valid sweep");
        assert_eq!(spec.blocks[0].design, DesignKind::Random { samples: 5 });
        assert_eq!(
            spec.blocks[0].design_seed, None,
            "derived from the root seed when planned"
        );

        for bad in ["lhs", "lhs:0", "grid:4", "lhs:x"] {
            let line = [
                "henad-cli",
                "sir",
                "--out",
                "d",
                "--vary",
                "infection_rate=0:1",
                "--sample",
                bad,
            ];
            assert_eq!(parse(&line), Err(ErrorKind::ValueValidation), "--sample {bad}");
        }
        for bad in ["-1", "soon", "NaN"] {
            let line = ["henad-cli", "sir", "--out", "d", &format!("--timeout={bad}")];
            assert_eq!(parse(&line), Err(ErrorKind::ValueValidation), "--timeout {bad}");
        }
        let unread = ["--stop", "Infected", "--reduce", "Infected:median"];
        for pair in unread.chunks(2) {
            let args = Args::parse_from(["henad-cli", "sir", "--out", "d", pair[0], pair[1]]);
            assert!(spec_from_flags(&args, "sir").is_err(), "{pair:?}");
        }
    }

    #[test]
    fn flags_make_one_block_over_every_vary() {
        let args = Args::parse_from([
            "henad-cli",
            "sir",
            "--out",
            "sweep",
            "--set",
            "grid_width=64",
            "--vary",
            "infection_rate=0.1,0.2",
            "--vary",
            "recovery_rate=0.1,0.2",
            "--zip",
            "--reps",
            "4",
            "--seed",
            "9",
            "--stats-every",
            "5",
            "--independent-seeds",
            "--reduce",
            "Infected:max",
            "--no-default-reducers",
        ]);
        let spec = spec_from_flags(&args, "sir").expect("a valid sweep");
        assert_eq!(spec.fixed, [("grid_width".to_owned(), "64".to_owned())]);
        assert_eq!((spec.run.steps, spec.run.warmup, spec.run.replicates), (1000, 0, 4));
        assert_eq!((spec.measure.stats_every, spec.measure.series_every), (5, 5));
        assert!(!spec.measure.default_reducers);
        assert_eq!(spec.measure.reducers.len(), 1);
        assert_eq!((spec.seeds.root, spec.seeds.scheme), (9, SeedScheme::Independent));
        assert_eq!(spec.blocks.len(), 1);
        assert_eq!(spec.blocks[0].design, DesignKind::Zip);
        assert_eq!(spec.blocks[0].factors.len(), 2);

        let lone = spec_from_flags(&Args::parse_from(["henad-cli", "sir", "--out", "sweep"]), "sir").expect("valid");
        assert!(lone.blocks.is_empty(), "no --vary runs the fixed values alone");
        assert_eq!((lone.seeds.root, lone.seeds.scheme), (0, SeedScheme::Common));
    }

    /// Returns the kind of error `line` gives, or `Ok` when it parses.
    fn parse(line: &[&str]) -> Result<(), ErrorKind> {
        Args::try_parse_from(line).map(|_| ()).map_err(|error| error.kind())
    }

    #[test]
    fn explore_flags_conflict_with_export() {
        let conflicts: [&[&str]; 13] = [
            &["henad-cli", "sir", "--out", "d", "--export-stats", "s.csv"],
            &["henad-cli", "sir", "--out", "d", "--export", "f.txt"],
            &["henad-cli", "sir", "--out", "d", "--global-warmup", "10"],
            &["henad-cli", "sir", "--dry-run", "--export-stats", "s.csv"],
            &[
                "henad-cli",
                "--spec",
                "s.toml",
                "--out",
                "d",
                "--act",
                "seed_outbreak@5",
            ],
            &["henad-cli", "--spec", "s.toml", "--out", "d", "--stop", "Infected <= 0"],
            &["henad-cli", "--spec", "s.toml", "--out", "d", "--timeout", "5"],
            &["henad-cli", "--spec", "s.toml", "--out", "d", "--design", "t.csv"],
            &[
                "henad-cli",
                "sir",
                "--out",
                "d",
                "--vary",
                "infection_rate=0:1",
                "--design",
                "t.csv",
            ],
            &[
                "henad-cli",
                "sir",
                "--out",
                "d",
                "--vary",
                "infection_rate=0:1",
                "--zip",
                "--sample",
                "lhs:4",
            ],
            &["henad-cli", "sir", "--out", "d", "--params"],
            &[
                "henad-cli",
                "--spec",
                "s.toml",
                "--out",
                "d",
                "--vary",
                "infection_rate=0.1,0.2",
            ],
            &["henad-cli", "--spec", "s.toml", "--out", "d", "--steps", "10"],
        ];
        for line in conflicts {
            assert_eq!(parse(line), Err(ErrorKind::ArgumentConflict), "{line:?}");
        }
    }

    #[test]
    fn explore_flags_need_a_sweep() {
        let missing: [&[&str]; 11] = [
            &["henad-cli", "sir", "--vary", "infection_rate=0.1,0.2"],
            &["henad-cli", "gpu_sir", "--gpu-memory", "1000000"],
            &["henad-cli", "sir", "--out", "d", "--zip"],
            &["henad-cli", "sir", "--series-every", "5"],
            &["henad-cli", "sir", "--concurrent", "2"],
            &["henad-cli", "sir", "--out", "d", "--sample", "lhs:4"],
            &["henad-cli", "sir", "--out", "d", "--design-seed", "3"],
            &["henad-cli", "sir", "--stop", "Infected <= 0"],
            &["henad-cli", "sir", "--shard", "0/2"],
            &["henad-cli", "sir", "--dry-run", "--resume"],
            &["henad-cli", "sir", "--out", "d", "--retry-failed"],
        ];
        for line in missing {
            assert_eq!(parse(line), Err(ErrorKind::MissingRequiredArgument), "{line:?}");
        }
    }

    #[test]
    fn explore_flags_combine_where_they_agree() {
        let accepted: [&[&str]; 10] = [
            &["henad-cli", "--spec", "s.toml", "--out", "d"],
            &[
                "henad-cli",
                "--spec",
                "s.toml",
                "--out",
                "d",
                "--shard",
                "1/2",
                "--resume",
                "--retry-failed",
            ],
            &["henad-cli", "sir", "--out", "d", "--act", "seed_outbreak@5"],
            &[
                "henad-cli",
                "sir",
                "--out",
                "d",
                "--design",
                "t.csv",
                "--act",
                "seed_outbreak@5",
            ],
            &[
                "henad-cli",
                "sir",
                "--out",
                "d",
                "--resume",
                "--dry-run",
                "--vary",
                "infection_rate=0:1",
                "--sample",
                "random:8",
                "--design-seed",
                "3",
                "--stop",
                "Infected <= 0",
                "--timeout",
                "0",
            ],
            &[
                "henad-cli",
                "sir",
                "--spec",
                "s.toml",
                "--dry-run",
                "--concurrent",
                "auto",
            ],
            &["henad-cli", "--spec", "s.toml", "--params", "--json"],
            &["henad-cli", "sir", "--vary", "infection_rate=0.1,0.2", "--dry-run"],
            &[
                "henad-cli",
                "sir",
                "--out",
                "d",
                "--reps",
                "3",
                "--seed",
                "4",
                "--memory",
                "1000000",
            ],
            &[
                "henad-cli",
                "gpu_sir",
                "--out",
                "d",
                "--concurrent",
                "4",
                "--gpu-memory",
                "1000000",
            ],
        ];
        for line in accepted {
            assert_eq!(parse(line), Ok(()), "{line:?}");
        }
    }

    #[test]
    fn a_sweep_exits_with_success_only_when_every_run_is_ok() {
        let counts = |ok, non_finite, failed| ResultCounts {
            rows: ok + non_finite + failed,
            ok,
            non_finite,
            failed,
        };
        let status = |end, counts: ResultCounts| exit_status(end, &counts).ok();
        assert_eq!(status(SweepEnd::Complete, counts(4, 0, 0)), Some(ExitCode::SUCCESS));
        assert_eq!(status(SweepEnd::Planned, counts(0, 0, 0)), Some(ExitCode::SUCCESS));
        let some_not_ok = Some(ExitCode::from(SOME_RUNS_NOT_OK));
        assert_eq!(status(SweepEnd::Complete, counts(3, 0, 1)), some_not_ok);
        assert_eq!(status(SweepEnd::Complete, counts(3, 1, 0)), some_not_ok);
        assert_eq!(status(SweepEnd::Aborted, counts(2, 0, 0)), None, "an abort is an error");
        assert_eq!(
            status(SweepEnd::DeviceLost, counts(2, 0, 0)),
            None,
            "a lost device is an error"
        );
    }

    #[test]
    fn sizes_and_durations_read_as_a_person_writes_them() {
        assert_eq!(format_bytes(512), "512 bytes");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(3 << 30), "3.0 GiB");
        assert_eq!(format_seconds(4.21), "4.2s");
        assert_eq!(format_seconds(42.4), "42s");
        assert_eq!(format_seconds(187.0), "3m 07s");
        assert_eq!(format_seconds(7500.0), "2h 05m");
    }

    #[test]
    fn a_search_spec_plans_its_budget_and_space() {
        // The spec sits in henad-explore's package, and a crate built from its tarball skips the test.
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../henad-explore/specs/sir_search_pse.toml");
        if !path.is_file() {
            eprintln!("note: skipped, {} is absent", path.display());
            return;
        }
        let loaded = LoadedSpec::read(&path).expect("the example spec reads");
        let models = example_models();
        let sir = models.get("sir").expect("sir is registered");
        let options = SweepOptions::new(provenance());
        let report = plan_spec(
            sir,
            None,
            &loaded.spec,
            None,
            &options,
            &mut henad_explore::progress::NoProgress,
        )
        .expect("the search plans");
        let text = plan_text("SIR", &report.outline, false);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "search of SIR (sir): pse, 1200 evaluations x 2 replicates = 2400 runs"
        );
        assert_eq!(lines[1], "  batch size:   64 candidates");
        assert_eq!(lines[2], "  axes:         Infected:max from 0 to 4096, 32 cells");
        assert_eq!(lines[3], "                Infected:argmax from 0 to 50, 20 cells");
        assert_eq!(lines[4], "  search space: infection_rate from 0.05 to 0.9");
        assert_eq!(lines[5], "                recovery_rate from 0.01 to 0.3");
        assert!(lines[6].starts_with("  search seed:  "), "{text}");
        assert_eq!(
            axis_text(&PatternAxis::automatic("Infected:max", 16)),
            "Infected:max, automatic range, 16 cells"
        );

        let args = Args::try_parse_from(["henad-cli", "--spec", "s.toml", "--dry-run"]).expect("a dry run parses");
        assert_eq!(Mode::of(&args), Mode::Explore);
    }

    /// Checks that a flag given on the command line replaces the spec table's setting, `--concurrent auto` included,
    /// and that a setting no flag gives keeps the table's value.
    #[test]
    fn a_build_change_prints_as_an_explore_warning_line() {
        use henad_explore::output::manifest::{BuildRole, RecordedBuild};
        use henad_explore::sweep::SweepWarning;

        let current = RecordedBuild::engine();
        let mut recorded = current.clone();
        recorded.version = "0.1.0".to_owned();
        let warning = SweepWarning::BuildChanged {
            role: BuildRole::Engine,
            recorded: Box::new(recorded),
            current: Box::new(current),
            between_shards: false,
        };
        let line = super::warning_json(&warning);
        assert_eq!(line["kind"], "explore_warning");
        assert_eq!(line["warning"], "build_changed");
        assert_eq!(line["role"], "engine");
        assert_eq!(line["recorded"]["version"], "0.1.0");
        assert_eq!(line["current"]["name"], "henad");
        assert_eq!(line["between_shards"], false);
        assert_eq!(line["message"], warning.to_string());
    }

    #[test]
    fn an_explicit_concurrent_auto_overrides_the_spec_table() {
        let dir = std::env::temp_dir().join(format!("henad-cli-execution-table-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory can be made");
        let path = dir.join("spec.toml");
        std::fs::write(
            &path,
            "model = \"sir\"\n[execution]\nconcurrent = 4\nmemory = 1000\ngpu_memory = 2000\n",
        )
        .expect("the spec can be written");
        let spec_arg = path.to_str().expect("the temporary directory is UTF-8");
        let entry = example_models().get("sir").cloned().expect("sir is registered");
        let options = |extra: &[&str]| {
            let mut line = vec!["henad-cli", "--spec", spec_arg, "--dry-run"];
            line.extend(extra);
            let args = Args::try_parse_from(line).expect("the line parses");
            let loaded = LoadedSpec::read(&path).expect("the spec reads");
            sweep_and_options(&args, &entry, Some(loaded)).expect("a sweep").1
        };

        let from_table = options(&[]);
        assert_eq!(from_table.concurrency.to_string(), "4");
        assert_eq!(
            (from_table.memory_budget, from_table.gpu_memory),
            (Some(1000), Some(2000))
        );

        let overridden = options(&["--concurrent", "auto", "--memory", "500"]);
        assert_eq!(overridden.concurrency, Concurrency::Auto);
        assert_eq!(
            (overridden.memory_budget, overridden.gpu_memory),
            (Some(500), Some(2000))
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
