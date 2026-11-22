//! Progress of a sweep or a merge as the text a person reads, or as `explore_*` JSON lines through
//! [`json_report`].

use std::io::{self, IsTerminal as _};
use std::time::{Duration, Instant};

use henad_core::explore::factor::{FactorSpec, FactorTarget, LevelSpec};
use henad_core::explore::outcome::{RunOutcome, RunStatus};
use henad_core::explore::plan::PlannedBlock;
use henad_core::explore::search::pse::PatternAxis;
use henad_core::explore::search::{Objective, RankingEntry};
use henad_core::explore::spec::ACTION_COLUMN_PREFIX;
use henad_explore::exec::ExecutionLayout;
use henad_explore::merge::MergeReport;
use henad_explore::progress::{Progress, ProgressEvent, ProgressUpdate};
use henad_explore::search_run::{SearchOutline, SearchUpdate};
use henad_explore::sweep::{SweepEnd, SweepOutline, SweepReport};

use crate::Args;
use crate::json_report;

/// Number of rows in `series.csv` above which the plan carries a warning.
pub const SERIES_ROWS_WARNING: u64 = 10_000_000;

/// Time between two progress lines when stderr is not a terminal.
const LOGGED_PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

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
    /// Number of runs that the output directory already holds. The sweep skips them.
    skipped: u64,
    /// Number of characters in the progress line on the terminal, 0 when no line is showing.
    shown_width: usize,
    last_logged: Option<Instant>,
    /// Outline of a search, `None` for a sweep.
    search: Option<SearchOutline>,
    /// Standing of a search after its latest batch.
    search_standing: Option<SearchStanding>,
}

/// Best candidate or filled cells of a search after a batch, and the number of evaluations told to the searcher.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SearchStanding {
    pub(crate) evaluations: u64,
    /// Best candidate, `None` for a Pattern Space Exploration.
    pub(crate) best: Option<RankingEntry>,
    /// Number of generations of a genetic algorithm finished so far.
    pub(crate) generation_count: u64,
    pub(crate) filled_cells: u64,
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
            json_report::emit(&json_report::explore_plan(outline));
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
            let message = format!(
                "series.csv will hold about {} rows. Increase {name} to write fewer, or set it to 0 to write none.",
                outline.series_rows
            );
            eprintln!("warning: {message}");
            if self.json {
                json_report::emit(&json_report::explore_series_rows_warning(&message));
            }
        }
    }

    fn committed(&mut self, outcome: &RunOutcome) {
        if self.json {
            json_report::emit(&json_report::explore_run(outcome));
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
            json_report::emit(&json_report::explore_progress(update, self.skipped));
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
        let finished = self
            .search_standing
            .as_ref()
            .map_or(0, |standing| standing.generation_count);
        let standing = self.search_standing.insert(SearchStanding {
            evaluations: update.evaluations,
            best: update.best.clone(),
            generation_count: finished + update.generations.len() as u64,
            filled_cells: update.filled_cells,
        });
        if self.json
            && let Some(search) = &self.search
        {
            json_report::emit(&json_report::explore_search_batch(search, update, standing));
        }
    }

    fn ended(&mut self, report: &SweepReport) {
        if self.json {
            json_report::emit(&json_report::explore_end(report, self.search_standing.as_ref()));
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
                eprintln!(
                    "wrote {} to {dir} in {elapsed} and skipped {}: {tally} in total",
                    plural(written, "run"),
                    plural(skipped, "run")
                );
            }
            SweepEnd::Complete => eprintln!("wrote {} to {dir} in {elapsed}: {tally}", plural(written, "run")),
            SweepEnd::Aborted => eprintln!("aborted after {}, written to {dir}", plural(written, "run")),
            SweepEnd::DeviceLost => eprintln!("GPU device lost after {}, written to {dir}", plural(written, "run")),
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
                    json_report::emit(&json_report::explore_warning(warning));
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
pub(super) struct WarningPrinter {
    pub(super) json: bool,
}

impl Progress for WarningPrinter {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        if let ProgressEvent::Warned(warning) = event {
            eprintln!("warning: {warning}");
            if self.json {
                json_report::emit(&json_report::explore_warning(warning));
            }
        }
    }
}

/// Returns the plan of a sweep over the model named `model_name`, as the lines a person reads.
///
/// A `resume` adds a line that counts the skipped runs and the runs to run.
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
    // A line with no label continues the list from the line above.
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
pub(crate) fn axis_text(axis: &PatternAxis) -> String {
    let cells = plural(u64::from(axis.cells), "cell");
    match axis.range() {
        Some((min, max)) => format!("{} from {min} to {max}, {cells}", axis.column),
        None => format!("{}, automatic range, {cells}", axis.column),
    }
}

/// Returns a factor of a search space as in `infection_rate from 0.05 to 0.9` or `network in Random, Geometric`.
pub(crate) fn factor_text(factor: &FactorSpec) -> String {
    let target = match &factor.target {
        FactorTarget::Param(id) => id.clone(),
        FactorTarget::Action(name) => format!("{ACTION_COLUMN_PREFIX}{name}"),
    };
    let (min, max, step, unit) = match &factor.levels {
        LevelSpec::Values(values) => return format!("{target} in {}", values.join(", ")),
        LevelSpec::All => return format!("{target}, all values"),
        &LevelSpec::Range { min, max, step } => (min, max, step, ""),
        &LevelSpec::PercentRange { min, max, step } => (min, max, step, "%"),
    };
    match step {
        None => format!("{target} from {min}{unit} to {max}{unit}"),
        Some(step) => format!("{target} from {min}{unit} to {max}{unit} in steps of {step}{unit}"),
    }
}

/// Returns the standing of `search` after a batch, as in
/// `40/480 evaluations, best candidate 17: Infected:max 102, over 8 replicates` or
/// `96/1200 evaluations, 45 cells filled`.
fn standing_text(search: &SearchOutline, standing: &SearchStanding) -> String {
    let evaluations = format!("{}/{} evaluations", standing.evaluations, search.max_evaluations);
    match (&search.objective, &standing.best) {
        (Some(objective), Some(best)) if best.objective.is_finite() => format!(
            "{evaluations}, best candidate {}: {} {}, over {}",
            best.candidate_id,
            objective.column,
            best.objective,
            plural(best.replicate_count, "replicate")
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
    let left = update.remaining.map_or_else(String::new, |remaining| {
        format!(", about {} left", format_seconds(remaining.as_secs_f64()))
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
        format_seconds(update.elapsed.as_secs_f64())
    )
}

/// Returns the end of a merge of `inputs` shard directories, as the line a person reads.
pub(super) fn merge_text(report: &MergeReport, inputs: usize) -> String {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser as _;
    use henad_core::explore::search::pse::PatternAxis;
    use henad_explore::spec_file::LoadedSpec;
    use henad_explore::sweep::{SweepOptions, plan_spec};
    use henad_models::example_models;

    use super::{axis_text, format_bytes, format_seconds, plan_text};
    use crate::{Args, Mode, test_provenance};

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
        let options = SweepOptions::new(test_provenance());
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
}
