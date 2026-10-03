//! Checks searches end to end: the example specs, the same tables at any concurrency, resume, the handle, the pumped
//! search a browser runs, and a search of a GPU model.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_compute::runner::{Pace, SimLoop as _};
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::search::genetic::GeneticSettings;
use henad_core::explore::search::hill_climb::HillClimbSettings;
use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use henad_core::explore::search::{Aggregate, Goal, Objective, SearchAlgorithm, SearchSpec};
use henad_core::explore::spec::{ActionSpec, SweepSpec};
use henad_core::explore::stop::StopSpec;
use henad_core::export::csv::{escape_field, parse_records};

use crate::cursor::{CursorState, RunCursor};
use crate::exec::{Concurrency, RunRequest, SweepControl};
use crate::handle::{SweepChannel, SweepEvent, SweepOutput, SweepRun, SweepRunOptions};
use crate::output::manifest::{BuildRole, Manifest, ManifestAxisRanges, ManifestMode, ManifestStatus, RecordedBuild};
use crate::output::resume::ResumeError;
use crate::output::search_tables::SearchHistory;
use crate::output::{
    ARCHIVE_FILE, BATCHES_FILE, BEST_FILE, EVALUATIONS_FILE, GENERATIONS_FILE, MANIFEST_FILE, OutputError, RUNS_FILE,
    SERIES_FILE, SUMMARY_FILE,
};
use crate::progress::{NoProgress, Progress, ProgressEvent};
use crate::pumped::PumpedSweep;
use crate::result_set::{ResultSet, ResultSetError};
use crate::search_run::{EvaluationReading, SearchPlan, SearchPlanError};
use crate::spec_file::SpecFile;
use crate::sweep::{ExploreError, SweepEnd, SweepOptions, SweepReport, SweepWarning};
use crate::tests::support::{
    CommitLimit, OutputTables, Recorder, ScratchDir, dry_run, entry, headless_device, other_engine, planned,
    provenance, rewrite_manifest, sweep, sweep_options, without_timing,
};

/// Longest a test waits for a search to end.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Tables a search writes beside the sweep's three.
const SEARCH_FILES: [&str; 5] = [
    EVALUATIONS_FILE,
    BATCHES_FILE,
    GENERATIONS_FILE,
    BEST_FILE,
    ARCHIVE_FILE,
];

fn fixed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect()
}

fn range(min: f64, max: f64) -> LevelSpec {
    LevelSpec::Range { min, max, step: None }
}

fn lane_count(count: usize) -> Concurrency {
    Concurrency::Fixed(NonZeroUsize::new(count).expect("a test lane count is above 0"))
}

fn genetic() -> SearchAlgorithm {
    SearchAlgorithm::Genetic(GeneticSettings {
        population: 8,
        reevaluate_fraction: 0.25,
        ..GeneticSettings::default()
    })
}

fn hill_climb() -> SearchAlgorithm {
    SearchAlgorithm::HillClimb(HillClimbSettings {
        patience: 2,
        reevaluate: true,
        ..HillClimbSettings::default()
    })
}

fn pattern() -> SearchAlgorithm {
    let axis = |column: &str, max, cells| PatternAxis::bounded(column, 0.0, max, cells);
    SearchAlgorithm::PatternSpaceExploration(PatternSpaceSettings {
        initial_samples: 10,
        ..PatternSpaceSettings::new(axis("Infected:max", 256.0, 8), axis("Infected:argmax", 40.0, 8))
    })
}

/// Returns [`pattern`] with an automatic range on each axis, fixed by the batch that holds candidate 9.
fn automatic_pattern() -> SearchAlgorithm {
    SearchAlgorithm::PatternSpaceExploration(PatternSpaceSettings {
        initial_samples: 10,
        ..PatternSpaceSettings::new(
            PatternAxis::automatic("Infected:max", 8),
            PatternAxis::automatic("Infected:argmax", 8),
        )
    })
}

/// Returns a search of SIR on a 16 by 16 grid with `algorithm`, 2 replicates of 40 steps per evaluation, batches of
/// 6 and a space over two rates and an action's tick.
fn search_spec(algorithm: SearchAlgorithm, max_evaluations: u64) -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "16"), ("grid_height", "16")]);
    spec.run.steps = 40;
    spec.run.replicates = 2;
    spec.run.stop = Some(StopSpec::parse("Infected <= 0", 10).expect("a valid condition"));
    spec.measure.stats_every = 4;
    spec.measure.series_every = 8;
    spec.measure.default_reducers = false;
    spec.measure.reducers = ["Infected:max", "Infected:argmax"]
        .map(|raw| raw.parse().expect("a valid reducer"))
        .to_vec();
    spec.seeds.root = 5;
    spec.actions = vec![ActionSpec {
        name: "wave".to_owned(),
        ..ActionSpec::new("seed_outbreak", 20)
    }];
    let objective = match algorithm {
        SearchAlgorithm::PatternSpaceExploration(_) => None,
        SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => Some(Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Minimize,
            aggregate: Aggregate::Median,
        }),
    };
    spec.search = Some(SearchSpec {
        algorithm,
        max_evaluations,
        batch_size: 6,
        objective,
        space: vec![
            FactorSpec::param("infection_rate", range(0.05, 0.9)),
            FactorSpec::param("recovery_rate", range(0.02, 0.3)),
            FactorSpec::action("wave", range(0.0, 40.0)),
        ],
    });
    spec
}

/// Runs the search `spec` over `entry` into `output_dir` with `options`, reporting to `progress`.
fn search_with(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    spec: &SweepSpec,
    output_dir: &Path,
    options: &SweepOptions,
    progress: &mut dyn Progress,
) -> Result<SweepReport, ExploreError> {
    crate::tests::support::sweep_with(entry, gpu, spec, output_dir, options, progress)
}

/// Runs the search `spec` into `output_dir` at `concurrency`, reporting nowhere.
///
/// # Panics
///
/// Panics when the search fails.
fn search(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    spec: &SweepSpec,
    output_dir: &Path,
    concurrency: Concurrency,
) -> SweepReport {
    let options = SweepOptions {
        concurrency,
        ..sweep_options(false)
    };
    search_with(entry, gpu, spec, output_dir, &options, &mut NoProgress).expect("the search runs")
}

/// Returns the search tables a search of `spec` writes, its closing table last.
///
/// # Panics
///
/// Panics when `spec` has no search.
fn written_search_files(spec: &SweepSpec) -> Vec<&'static str> {
    let algorithm = &spec.search.as_ref().expect("a search spec").algorithm;
    let mut files = vec![EVALUATIONS_FILE, BATCHES_FILE];
    if matches!(algorithm, SearchAlgorithm::Genetic(_)) {
        files.push(GENERATIONS_FILE);
    }
    files.push(match algorithm {
        SearchAlgorithm::PatternSpaceExploration(_) => ARCHIVE_FILE,
        SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => BEST_FILE,
    });
    files
}

/// Every table of a search's output directory, with the timing columns of `runs.csv` emptied.
#[derive(Debug, PartialEq, Eq)]
struct SearchTables {
    tables: OutputTables,
    /// Text of each search table the search wrote, by file name.
    search_tables: Vec<(&'static str, String)>,
}

impl SearchTables {
    /// Reads the tables a search of `spec` wrote to `dir`.
    ///
    /// # Panics
    ///
    /// Panics when a table the search writes is missing, or a table it does not write is there.
    fn read(dir: &Path, spec: &SweepSpec) -> Self {
        let written = written_search_files(spec);
        for file in SEARCH_FILES.into_iter().filter(|file| !written.contains(file)) {
            assert!(!dir.join(file).exists(), "a search of this kind writes no {file}");
        }
        let search_tables = written
            .into_iter()
            .map(|file| {
                let text = std::fs::read_to_string(dir.join(file))
                    .unwrap_or_else(|error| panic!("the search wrote {file}: {error}"));
                (file, text)
            })
            .collect();
        Self {
            tables: OutputTables::read(dir),
            search_tables,
        }
    }

    fn table(&self, file: &str) -> Vec<Vec<String>> {
        let text = self
            .search_tables
            .iter()
            .find(|(name, _)| *name == file)
            .map(|(_, text)| text)
            .unwrap_or_else(|| panic!("the search wrote {file}"));
        parse_records(text).expect("the table is valid CSV")
    }
}

#[test]
fn every_example_search_spec_parses() {
    let specs = Path::new(env!("CARGO_MANIFEST_DIR")).join("specs");
    let models = henad_models::example_models();
    let mut planned = Vec::new();
    for file in std::fs::read_dir(&specs).expect("the specs directory exists") {
        let path = file.expect("the directory lists").path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.starts_with("sir_search_") {
            continue;
        }
        let (file, _) = SpecFile::load(&path).expect("an example spec parses");
        let spec = file.into_spec().expect("an example spec is a search spec");
        let written = SpecFile::from(&spec).to_toml().expect("a spec file serializes");
        let back = SpecFile::parse(&written)
            .and_then(SpecFile::into_spec)
            .expect("the written spec reads back");
        assert_eq!(back, spec, "{name} survives a round trip through TOML");
        let entry = models
            .get(&spec.model)
            .expect("an example spec names a registered model");
        let plan =
            SearchPlan::new(&spec, &entry.schema()).unwrap_or_else(|error| panic!("{name} does not plan: {error:?}"));
        let search = plan.search();
        planned.push((
            name,
            search.algorithm.as_str(),
            search.max_evaluations,
            plan.run_count(),
        ));
    }
    planned.sort();
    assert_eq!(
        planned,
        [
            ("sir_search_genetic.toml".to_owned(), "genetic", 480, 1920),
            ("sir_search_pse.toml".to_owned(), "pse", 1200, 2400),
        ],
        "the counts each header comment gives"
    );
}

#[test]
fn a_search_writes_the_same_tables_at_any_concurrency() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-any-concurrency");
    // A generation of 8 asks for batches of 6 and 2, and the other searches for batches of 6. A PSE cuts its second
    // batch at the last of its 10 initial samples.
    for (name, algorithm, batches) in [
        ("genetic", genetic(), 7),
        ("hill", hill_climb(), 5),
        ("pse", pattern(), 6),
    ] {
        let spec = search_spec(algorithm, 30);
        let tables: Vec<SearchTables> = [1, 3]
            .into_iter()
            .map(|count| {
                let output_dir = scratch.path().join(format!("{name}-{count}"));
                let report = search(&sir, None, &spec, &output_dir, lane_count(count));
                assert_eq!(report.end, SweepEnd::Complete);
                assert_eq!(report.outline.layout.cpu_lanes, count, "{name}");
                assert_eq!(report.counts.rows, 60, "{name}: 30 evaluations of 2 replicates");
                SearchTables::read(&output_dir, &spec)
            })
            .collect();
        let first = &tables[0];
        let closing = *written_search_files(&spec)
            .last()
            .expect("a search writes a closing table");
        assert!(first.table(closing).len() > 1, "{name}: {closing} holds the candidates");
        assert_eq!(first.tables.runs.len(), 1 + 60, "{name}");
        assert_eq!(first.table(EVALUATIONS_FILE).len(), 1 + 30, "{name}");
        assert_eq!(first.table(BATCHES_FILE).len(), 1 + batches, "{name}");
        let config_ids = first.tables.run_column("config_id");
        let replicate_indices = first.tables.run_column("rep");
        assert_eq!(
            (config_ids[2], replicate_indices[2]),
            ("1", "0"),
            "{name}: the runs of candidate 1 follow those of 0"
        );
        assert_eq!(tables[1], *first, "{name}: 3 lanes against 1");
    }
    let genetic_tables = SearchTables::read(&scratch.path().join("genetic-1"), &search_spec(genetic(), 30));
    assert!(
        genetic_tables.table(GENERATIONS_FILE).len() > 1,
        "a genetic algorithm writes its generations"
    );
    assert!(genetic_tables.table(BEST_FILE).len() > 1);
    let pattern_tables = SearchTables::read(&scratch.path().join("pse-1"), &search_spec(pattern(), 30));
    let archive = pattern_tables.table(ARCHIVE_FILE);
    assert!(archive.len() > 2, "the exploration fills cells");
    let hits: u64 = archive[1..]
        .iter()
        .map(|row| row[6].parse::<u64>().expect("a hit count"))
        .sum();
    assert_eq!(hits, 30, "every evaluation lands in one cell");
}

#[test]
fn a_reevaluation_gets_fresh_replicate_indices_and_seeds() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-reevaluation");
    let spec = search_spec(genetic(), 30);
    search(&sir, None, &spec, scratch.path(), lane_count(1));
    let tables = SearchTables::read(scratch.path(), &spec);
    let evaluations = tables.table(EVALUATIONS_FILE);
    let column = |name: &str| {
        evaluations[0]
            .iter()
            .position(|header| header == name)
            .unwrap_or_else(|| panic!("evaluations.csv has no column '{name}'"))
    };
    let (origin, offset, first_parent, second_parent, reevaluated, pooled) = (
        column("origin"),
        column("replicate_offset"),
        column("first_parent_id"),
        column("second_parent_id"),
        column("reevaluated_id"),
        column("pooled_replicates"),
    );
    let reevaluations: Vec<&Vec<String>> = evaluations[1..]
        .iter()
        .filter(|row| row[origin] == "reevaluation")
        .collect();
    assert!(
        !reevaluations.is_empty(),
        "the genetic algorithm re-evaluates its best members"
    );
    let seeds = tables.tables.run_column("seed");
    let replicate_indices = tables.tables.run_column("rep");
    let config_ids = tables.tables.run_column("config_id");
    for row in reevaluations {
        assert_eq!(
            (row[first_parent].as_str(), row[second_parent].as_str()),
            ("", ""),
            "a re-evaluation has no parents"
        );
        let replicate_offset: u64 = row[offset].parse().expect("an offset");
        assert!(
            replicate_offset >= 2,
            "a re-evaluation starts past its candidate's replicates"
        );
        assert_eq!(row[pooled].parse::<u64>().expect("a count"), replicate_offset + 2);
        let candidate_id = &row[0];
        let repeated = &row[reevaluated];
        let runs_of = |id: &str| -> Vec<(&str, &str)> {
            config_ids
                .iter()
                .zip(replicate_indices.iter().zip(&seeds))
                .filter(|(config_id, _)| **config_id == id)
                .map(|(_, (replicate, seed))| (*replicate, *seed))
                .collect()
        };
        let fresh = runs_of(candidate_id);
        let first = runs_of(repeated);
        assert_eq!(
            fresh[0].0, row[offset],
            "the first run takes the offset as its replicate index"
        );
        assert!(
            fresh
                .iter()
                .all(|(_, seed)| first.iter().all(|(_, earlier)| earlier != seed)),
            "candidate {candidate_id} repeats {repeated} with fresh seeds"
        );
    }
}

/// Checks that a dry run of a resume counts the runs a search folder holds and the runs its resume would add, and
/// leaves every file as it was.
#[test]
fn a_dry_run_counts_the_runs_a_search_resume_skips() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-resume-dry-run");
    let spec = search_spec(genetic(), 30);
    let control = SweepControl::new();
    let interrupted = SweepOptions {
        concurrency: lane_count(1),
        control: control.clone(),
        ..sweep_options(false)
    };
    let mut abort = CommitLimit::new(control, 17);
    let report =
        search_with(&sir, None, &spec, scratch.path(), &interrupted, &mut abort).expect("an abort is not an error");
    assert_eq!(report.end, SweepEnd::Aborted);
    let kept = report.counts.rows;
    let files = |dir: &Path| -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
            .expect("the folder reads")
            .map(|file| {
                let path = file.expect("an entry").path();
                let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
                (name, std::fs::read(&path).expect("the file reads"))
            })
            .collect();
        files.sort();
        files
    };
    let before = files(scratch.path());

    let report =
        dry_run(&sir, &spec, Some(scratch.path()), &sweep_options(true), &mut NoProgress).expect("the dry run plans");
    assert_eq!(report.end, SweepEnd::Planned);
    assert_eq!((report.outline.skipped, report.outline.pending), (kept, 60 - kept));
    assert_eq!(files(scratch.path()), before, "a dry run leaves the folder as it was");

    let error = dry_run(
        &sir,
        &spec,
        Some(scratch.path()),
        &sweep_options(false),
        &mut NoProgress,
    )
    .expect_err("a folder with results and no resume");
    assert!(
        matches!(error, ExploreError::Output(OutputError::HoldsResults { .. })),
        "{error:?}"
    );
}

#[test]
fn a_search_resume_under_another_engine_build_warns() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-resume-build");
    let spec = search_spec(SearchAlgorithm::Random, 12);
    let control = SweepControl::new();
    let interrupted = SweepOptions {
        concurrency: lane_count(1),
        control: control.clone(),
        ..sweep_options(false)
    };
    let mut abort = CommitLimit::new(control, 5);
    search_with(&sir, None, &spec, scratch.path(), &interrupted, &mut abort).expect("an abort is not an error");

    let resume = SweepOptions {
        provenance: provenance().with_engine(other_engine()),
        ..sweep_options(true)
    };
    let mut progress = Recorder::default();
    let report = search_with(&sir, None, &spec, scratch.path(), &resume, &mut progress).expect("the search resumes");
    assert_eq!(report.end, SweepEnd::Complete);
    assert_eq!(
        progress.warnings,
        [SweepWarning::BuildChanged {
            role: BuildRole::Engine,
            recorded: Box::new(RecordedBuild::engine()),
            current: Box::new(other_engine()),
            between_shards: false,
        }]
    );
    let manifest = Manifest::read(&scratch.path().join(MANIFEST_FILE)).expect("the manifest reads back");
    assert_eq!(
        manifest.recorded_builds(BuildRole::Engine),
        [RecordedBuild::engine(), other_engine()]
    );
}

#[test]
fn a_resumed_search_follows_the_same_trajectory() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-resume");
    for (name, algorithm, batches) in [
        ("genetic", genetic(), 7),
        ("pse", pattern(), 6),
        ("pse-automatic", automatic_pattern(), 6),
    ] {
        let spec = search_spec(algorithm, 30);
        let fresh_dir = scratch.path().join(format!("{name}-fresh"));
        search(&sir, None, &spec, &fresh_dir, lane_count(2));
        let fresh = SearchTables::read(&fresh_dir, &spec);

        let resumed_dir = scratch.path().join(format!("{name}-resumed"));
        let control = SweepControl::new();
        let interrupted = SweepOptions {
            concurrency: lane_count(2),
            control: control.clone(),
            ..sweep_options(false)
        };
        let mut abort = CommitLimit::new(control, 17);
        let report =
            search_with(&sir, None, &spec, &resumed_dir, &interrupted, &mut abort).expect("an abort is not an error");
        assert_eq!(report.end, SweepEnd::Aborted, "{name}");
        let kept = report.counts.rows;
        assert!((17..60).contains(&kept), "{name}: {kept} runs written");
        // A process killed mid-write leaves a partial row and the series of a run it never recorded.
        append(&resumed_dir.join(RUNS_FILE), "999,40,0,1,");
        append(&resumed_dir.join(SERIES_FILE), &format!("{kept},0,1,2,3\n{kept},4,1,2"));

        let resume = SweepOptions {
            concurrency: lane_count(3),
            ..sweep_options(true)
        };
        let report =
            search_with(&sir, None, &spec, &resumed_dir, &resume, &mut NoProgress).expect("the search resumes");
        assert_eq!(
            (report.end, report.outline.skipped),
            (SweepEnd::Complete, kept),
            "{name}"
        );
        assert_eq!(report.counts.rows, 60);
        assert_eq!(
            SearchTables::read(&resumed_dir, &spec),
            fresh,
            "{name}: the resumed tables equal a fresh search's"
        );
        let manifest = Manifest::read(&resumed_dir.join(MANIFEST_FILE)).expect("the manifest reads back");
        assert_eq!(
            (manifest.mode, manifest.status),
            (ManifestMode::Search, ManifestStatus::Complete)
        );
        assert_eq!(manifest.sessions.len(), 2);
        assert_eq!(
            manifest.plan.configs, None,
            "{name}: a search has a budget, not a count of configs"
        );
        let recorded = manifest.search.expect("a search records its standing");
        assert_eq!((recorded.evaluations, recorded.batch_count), (30, batches), "{name}");
    }
}

fn append(path: &Path, text: &str) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("the table opens");
    file.write_all(text.as_bytes()).expect("the table takes the text");
}

#[test]
fn a_resume_that_meets_a_changed_run_leaves_the_directory_alone() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-resume-changed-run");
    let spec = search_spec(genetic(), 12);
    search(&sir, None, &spec, scratch.path(), lane_count(1));
    // The last run gets another key, as under a build whose last batch decodes differently.
    let runs_path = scratch.path().join(RUNS_FILE);
    let text = std::fs::read_to_string(&runs_path).expect("runs.csv reads");
    let mut records = parse_records(&text).expect("runs.csv is valid CSV");
    let run_key = records[0]
        .iter()
        .position(|header| header == "run_key")
        .expect("runs.csv has a run_key column");
    let last = records.last_mut().expect("runs.csv holds runs");
    let changed_run_id: u64 = last[0].parse().expect("a run id");
    last[run_key] = "0".to_owned();
    let lines: Vec<String> = records
        .iter()
        .map(|record| {
            let fields: Vec<String> = record.iter().map(|field| escape_field(field)).collect();
            format!("{}\n", fields.join(","))
        })
        .collect();
    std::fs::write(&runs_path, lines.concat()).expect("runs.csv writes");
    let before = SearchTables::read(scratch.path(), &spec);
    let manifest_before = std::fs::read(scratch.path().join(MANIFEST_FILE)).expect("the manifest reads");

    let resume = sweep_options(true);
    let error = search_with(&sir, None, &spec, scratch.path(), &resume, &mut NoProgress).expect_err("a changed run");
    assert!(
        matches!(
            error,
            ExploreError::Resume(ResumeError::SearchRunChanged { run_id }) if run_id == changed_run_id
        ),
        "{error:?}"
    );
    assert_eq!(
        SearchTables::read(scratch.path(), &spec),
        before,
        "every table is left as it was"
    );
    assert_eq!(
        std::fs::read(scratch.path().join(MANIFEST_FILE)).expect("the manifest reads"),
        manifest_before,
        "the manifest is left as it was"
    );
}

#[test]
fn a_resume_refuses_runs_past_the_budget() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-resume-past-budget");
    let spec = search_spec(genetic(), 12);
    search(&sir, None, &spec, scratch.path(), lane_count(1));
    let runs_path = scratch.path().join(RUNS_FILE);
    let text = std::fs::read_to_string(&runs_path).expect("runs.csv reads");
    let row_count = text.lines().count() - 1;
    assert_eq!(row_count, 24, "the budget of 12 evaluations of 2 replicates is spent");
    let last = text.lines().last().expect("runs.csv holds runs");
    let (_, rest) = last.split_once(',').expect("a run id field");
    append(&runs_path, &format!("{row_count},{rest}\n"));
    let before = std::fs::read(&runs_path).expect("runs.csv reads");

    let resume = sweep_options(true);
    let error = search_with(&sir, None, &spec, scratch.path(), &resume, &mut NoProgress).expect_err("a run too many");
    assert!(
        matches!(error, ExploreError::Resume(ResumeError::UnknownRun { run_id }) if run_id == 24),
        "{error:?}"
    );
    assert_eq!(
        std::fs::read(&runs_path).expect("runs.csv reads"),
        before,
        "runs.csv is left as it was"
    );
}

#[test]
fn a_search_resume_names_a_changed_model() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-resume-model");
    let spec = search_spec(genetic(), 12);
    search(&sir, None, &spec, scratch.path(), lane_count(1));
    // Another version of the model changes the schema hash, and with it the search hash.
    rewrite_manifest(scratch.path(), |recorded| {
        recorded.model.schema_hash = "0000000000000000".to_owned();
        if let Some(search) = &mut recorded.search {
            search.search_hash = "0000000000000000".to_owned();
        }
    });

    let resume = sweep_options(true);
    let error = search_with(&sir, None, &spec, scratch.path(), &resume, &mut NoProgress).expect_err("another model");
    assert!(
        matches!(error, ExploreError::Resume(ResumeError::SchemaChanged { .. })),
        "{error:?}"
    );
}

#[test]
fn a_resume_refuses_another_search_or_a_sweep() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-resume-refused");
    let spec = search_spec(genetic(), 12);
    let search_dir = scratch.path().join("search");
    search(&sir, None, &spec, &search_dir, lane_count(1));
    let resume = sweep_options(true);

    let mut changed = spec.clone();
    if let Some(SearchSpec {
        algorithm: SearchAlgorithm::Genetic(settings),
        ..
    }) = &mut changed.search
    {
        settings.mutation_rate = 0.5;
    }
    let error = search_with(&sir, None, &changed, &search_dir, &resume, &mut NoProgress).expect_err("another search");
    assert!(
        matches!(error, ExploreError::Resume(ResumeError::SearchChanged { .. })),
        "{error:?}"
    );

    let mut sweep_spec = spec.clone();
    sweep_spec.search = None;
    let sweep_dir = scratch.path().join("sweep");
    sweep(&sir, None, &sweep_spec, &sweep_dir, lane_count(1));
    let error = search_with(&sir, None, &spec, &sweep_dir, &resume, &mut NoProgress).expect_err("a sweep");
    assert!(
        matches!(
            error,
            ExploreError::Resume(ResumeError::ModeChanged {
                recorded: ManifestMode::Sweep,
                current: ManifestMode::Search
            })
        ),
        "{error:?}"
    );
    let error = crate::tests::support::sweep_with(&sir, None, &sweep_spec, &search_dir, &resume, &mut NoProgress)
        .expect_err("a search resumed as a sweep");
    assert!(
        matches!(error, ExploreError::Resume(ResumeError::ModeChanged { .. })),
        "{error:?}"
    );

    let merged = crate::merge::merge(
        std::slice::from_ref(&search_dir),
        &scratch.path().join("merged"),
        &mut NoProgress,
    )
    .expect_err("a search has no shards");
    assert!(
        matches!(merged, crate::merge::MergeError::NotASweep { .. }),
        "{merged:?}"
    );

    let retry = SweepOptions {
        retry_failed: true,
        ..resume.clone()
    };
    let error = search_with(&sir, None, &spec, &search_dir, &retry, &mut NoProgress).expect_err("a retry");
    assert!(
        matches!(error, ExploreError::Search(SearchPlanError::RetryFailed)),
        "{error:?}"
    );
}

#[test]
fn a_watched_column_must_name_a_reducer() {
    let sir = entry("sir", None);
    let mut spec = search_spec(genetic(), 12);
    if let Some(search) = &mut spec.search {
        search.objective = Some(Objective {
            column: "Recovered:max".to_owned(),
            goal: Goal::Maximize,
            aggregate: Aggregate::Mean,
        });
    }
    let error = dry_run(&sir, &spec, None, &sweep_options(false), &mut NoProgress).expect_err("no such reducer");
    let ExploreError::Search(SearchPlanError::UnknownColumn { column, known }) = error else {
        panic!("{error:?}");
    };
    assert_eq!(column, "Recovered:max");
    assert_eq!(known, ["Infected:max", "Infected:argmax"]);
}

/// Progress that puts a directory where `best.csv` goes once a batch is told. The search then fails as it ends.
struct ClosingTableBlocker {
    output_dir: PathBuf,
}

impl Progress for ClosingTableBlocker {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        let path = self.output_dir.join(BEST_FILE);
        if matches!(event, ProgressEvent::SearchBatchTold(_)) && !path.exists() {
            std::fs::create_dir(&path).expect("the directory is created");
        }
    }
}

#[test]
fn a_failed_search_records_its_standing_in_the_manifest() {
    let scratch = ScratchDir::new("search-failed-standing");
    let mut blocker = ClosingTableBlocker {
        output_dir: scratch.path().to_owned(),
    };
    let error = search_with(
        &entry("sir", None),
        None,
        &search_spec(SearchAlgorithm::Random, 12),
        scratch.path(),
        &sweep_options(false),
        &mut blocker,
    )
    .expect_err("best.csv cannot be written");
    assert!(
        matches!(&error, ExploreError::Output(OutputError::Write { path, .. }) if path.ends_with(BEST_FILE)),
        "{error:?}"
    );

    let manifest = Manifest::read(&scratch.path().join(MANIFEST_FILE)).expect("the manifest reads back");
    assert_eq!(manifest.status, ManifestStatus::Failed);
    let search = manifest.search.expect("a search records its standing");
    assert_eq!((search.evaluations, search.batch_count), (12, 2));
    assert!(search.best_candidate_id.is_some());
    for (file, rows) in [(EVALUATIONS_FILE, 12), (BATCHES_FILE, 2)] {
        let table = std::fs::read_to_string(scratch.path().join(file)).expect("the table reads");
        assert_eq!(table.lines().count(), 1 + rows, "{file}");
    }
}

/// Receives the events of `run` until its last one, and returns them.
fn drain(run: &mut SweepRun) -> Vec<SweepEvent> {
    let deadline = Instant::now() + SEARCH_TIMEOUT;
    let mut events = Vec::new();
    while !run.is_ended() {
        assert!(Instant::now() < deadline, "the search did not end in time");
        run.update(0.0);
        match run.try_recv() {
            Some(event) => events.push(event),
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    events
}

/// Text of every table of `files`, with the timing columns of `runs.csv` emptied.
fn untimed(files: &[(String, Vec<u8>)]) -> Vec<(String, String)> {
    files
        .iter()
        .filter(|(name, _)| name != MANIFEST_FILE)
        .map(|(name, bytes)| {
            let text = String::from_utf8(bytes.clone()).expect("the files are UTF-8");
            if name == RUNS_FILE {
                let runs = without_timing(parse_records(&text).expect("runs.csv is valid CSV"));
                (name.clone(), format!("{runs:?}"))
            } else {
                (name.clone(), text)
            }
        })
        .collect()
}

/// Returns the files of the output directory `dir`, in the order a search held in memory lists them.
fn directory_files(dir: &Path, algorithm_files: &[&str]) -> Vec<(String, Vec<u8>)> {
    [RUNS_FILE, SERIES_FILE, SUMMARY_FILE, MANIFEST_FILE]
        .iter()
        .chain(algorithm_files)
        .map(|file| {
            (
                (*file).to_owned(),
                std::fs::read(dir.join(file)).expect("the file is written"),
            )
        })
        .collect()
}

#[test]
fn a_search_through_a_handle_sends_each_batch_after_its_runs() {
    let sir = entry("sir", None);
    let spec = search_spec(pattern(), 18);
    let mut options = SweepRunOptions::new(provenance());
    options.concurrency = lane_count(2);
    let mut run = SweepRun::start(sir, None, spec.clone(), SweepOutput::Memory, options).expect("the search starts");
    assert_eq!(
        run.progress().runs_total,
        36,
        "a search's runs are known before it is planned"
    );
    assert!(run.search_plan().is_some());
    let events = drain(&mut run);
    let mut runs_seen = 0;
    let mut run_candidates = Vec::new();
    let mut batches = Vec::new();
    for event in &events {
        match event {
            SweepEvent::RunFinished { outcome, .. } => {
                run_candidates.push(outcome.run.config_id);
                runs_seen += 1;
            }
            SweepEvent::SearchBatchTold(update) => {
                assert_eq!(update.runs, runs_seen, "a batch follows its runs");
                run_candidates.dedup();
                let told: Vec<u64> = update
                    .evaluated
                    .iter()
                    .map(|evaluated| evaluated.candidate_id)
                    .collect();
                assert_eq!(
                    std::mem::take(&mut run_candidates),
                    told,
                    "runs come before their batch"
                );
                assert!(
                    update
                        .evaluated
                        .iter()
                        .all(|evaluated| matches!(evaluated.reading, EvaluationReading::Placement { .. }))
                );
                batches.push(update.batch);
            }
            SweepEvent::Planned(outline) => {
                assert_eq!(outline.search.as_ref().map(|search| search.algorithm), Some("pse"));
            }
            SweepEvent::Finished(_) | SweepEvent::Failed(_) | SweepEvent::Warned(_) => {}
        }
    }
    assert_eq!(
        batches,
        [0, 1, 2, 3],
        "batches of 6 and 4 initial samples, then of 6 and 2"
    );
    let Some(SweepEvent::Finished(record)) = events.last() else {
        panic!("the search did not finish: {:?}", events.last());
    };
    let report = record.search.as_ref().expect("a search reports its archive");
    assert!(!report.archive.is_empty());
    let files = record.files.clone().expect("files in memory").into_entries();

    let scratch = ScratchDir::new("search-handle");
    search(&entry("sir", None), None, &spec, scratch.path(), lane_count(1));
    let directory = directory_files(scratch.path(), &[EVALUATIONS_FILE, BATCHES_FILE, ARCHIVE_FILE]);
    assert_eq!(untimed(&files), untimed(&directory));
}

#[test]
fn the_pumped_search_writes_what_a_directory_search_writes() {
    let spec = search_spec(genetic(), 18);
    let sir = entry("sir", None);
    let search_plan = Arc::new(SearchPlan::new(&spec, &sir.schema()).expect("a valid search"));
    let options = || SweepRunOptions::new(provenance());
    let (channel, events, _) = SweepChannel::open(&options());
    let mut pumped = PumpedSweep::new(
        sir,
        spec.clone(),
        Arc::clone(search_plan.base()),
        Some(search_plan),
        channel,
        options(),
    )
    .expect("a CPU model pumps");
    let deadline = Instant::now() + SEARCH_TIMEOUT;
    while matches!(pumped.pump(), Pace::Now) {
        assert!(Instant::now() < deadline, "the pumped search never went idle");
    }
    let received: Vec<SweepEvent> = events.try_iter().collect();
    let searched = received
        .iter()
        .filter(|event| matches!(event, SweepEvent::SearchBatchTold(_)))
        .count();
    assert_eq!(
        searched, 5,
        "batches of 6, 2, 6, 2 and the last 2 over generations of 8"
    );
    let Some(SweepEvent::Finished(record)) = received.last() else {
        panic!("the pumped search did not finish");
    };
    assert_eq!(record.report.counts.rows, 36);
    let files = record.files.clone().expect("files in memory").into_entries();

    let scratch = ScratchDir::new("search-pumped");
    search(&entry("sir", None), None, &spec, scratch.path(), lane_count(1));
    let directory = directory_files(
        scratch.path(),
        &[EVALUATIONS_FILE, BATCHES_FILE, GENERATIONS_FILE, BEST_FILE],
    );
    assert_eq!(untimed(&files), untimed(&directory));
}

#[test]
fn a_gpu_search_writes_the_same_tables_on_any_track_count() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let mut spec = search_spec(SearchAlgorithm::Random, 12);
    spec.model = "gpu_sir".to_owned();
    spec.fixed = fixed(&[("grid_width", "32"), ("grid_height", "32")]);
    let scratch = ScratchDir::new("search-gpu");
    let tables: Vec<SearchTables> = [1, 3]
        .into_iter()
        .map(|count| {
            let output_dir = scratch.path().join(format!("tracks-{count}"));
            let report = search(&gpu_sir, Some(&ctx), &spec, &output_dir, lane_count(count));
            assert_eq!(report.outline.layout.gpu_tracks, count);
            assert_eq!((report.counts.rows, report.counts.ok), (24, 24));
            SearchTables::read(&output_dir, &spec)
        })
        .collect();
    assert_eq!(tables[0].table(BEST_FILE).len(), 1 + 12, "every candidate is ranked");
    assert_eq!(tables[1], tables[0], "3 tracks against 1");
}

/// Progress that keeps the course of a search from its batches.
#[derive(Default)]
struct HistoryRecorder(SearchHistory);

impl Progress for HistoryRecorder {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        if let ProgressEvent::SearchBatchTold(update) = event {
            self.0.push(update);
        }
    }
}

/// Checks the tables of a Pattern Space Exploration named `name` in `set` against `history`, read from them.
///
/// Every evaluation lands in a cell, the manifest and `archive.csv` give the ranges `history` holds, and under an
/// automatic range only the batch before it was taken goes without cells.
fn check_pattern_tables(name: &str, set: &ResultSet, history: &SearchHistory) {
    let settings = history.pattern_settings.as_ref().expect("the axes have their ranges");
    let recorded = set.manifest().search.as_ref().and_then(|search| search.axis_ranges);
    assert_eq!(recorded, ManifestAxisRanges::of(settings), "{name}");
    assert_eq!(
        history.archive.values().map(|entry| entry.hits).sum::<u64>(),
        18,
        "{name}: every evaluation lands in a cell"
    );
    let archive = parse_records(set.search_table(ARCHIVE_FILE).expect("the archive is written"))
        .expect("the archive is valid CSV");
    for row in &archive[1..] {
        let bound = |column: usize| row[column].parse::<f64>().expect("a bound");
        let index = |column: usize| row[column].parse::<u32>().expect("an index");
        assert_eq!(
            settings.x_axis.cell_bounds(index(0)),
            Some((bound(2), bound(3))),
            "{name}"
        );
        assert_eq!(
            settings.y_axis.cell_bounds(index(1)),
            Some((bound(4), bound(5))),
            "{name}"
        );
    }
    if name == "pse-automatic" {
        let evaluations = parse_records(set.search_table(EVALUATIONS_FILE).expect("the table is written"))
            .expect("the table is valid CSV");
        let column = |header: &str| {
            evaluations[0]
                .iter()
                .position(|name| name == header)
                .expect("the table has the column")
        };
        let (batch, x, x_index) = (column("batch"), column("x"), column("x_index"));
        let (outside, new_cell) = (column("outside"), column("new_cell"));
        for row in &evaluations[1..] {
            assert!(!row[x].is_empty(), "every evaluation has its outputs");
            assert_eq!(
                row[x_index].is_empty(),
                row[batch] == "0",
                "only the batch before the range is fixed goes without cells: {row:?}"
            );
            assert_eq!(
                [row[outside].is_empty(), row[new_cell].is_empty()],
                [row[x_index].is_empty(); 2],
                "a row without a cell leaves outside and new_cell empty: {row:?}"
            );
        }
    }
}

#[test]
fn a_search_folder_reads_back_with_its_history_and_replays_its_runs() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("search-result-set");
    for (name, algorithm) in [
        ("genetic", genetic()),
        ("pse", pattern()),
        ("pse-automatic", automatic_pattern()),
    ] {
        let spec = search_spec(algorithm, 18);
        let output_dir = scratch.path().join(name);
        let options = SweepOptions {
            concurrency: lane_count(2),
            ..sweep_options(false)
        };
        let mut recorder = HistoryRecorder::default();
        search_with(&sir, None, &spec, &output_dir, &options, &mut recorder).expect("the search runs");
        let set = ResultSet::open_dir(&output_dir, usize::MAX).expect("the folder reads back");
        assert!(set.is_search(), "{name}");
        let history = set
            .search_history()
            .expect("the tables read")
            .expect("a search has a history");
        assert_eq!(history, recorder.0, "{name}: the tables hold what the events reported");
        assert!(set.search_table(EVALUATIONS_FILE).is_some());
        if name.starts_with("pse") {
            check_pattern_tables(name, &set, &history);
        }
        let (_, measure) = planned(&sir, None, &spec);
        for run_id in [0, 7, 35] {
            let row = set.run(run_id).expect("the run is in the folder");
            let replay = set.replay(sir.schema(), run_id).expect("a search run replays");
            assert_eq!(replay.seed, row.outcome.run.seed);
            let request = RunRequest {
                run: row.outcome.run,
                run_key: row.outcome.run_key,
                params: &replay.params,
                schedule: replay.schedule.clone(),
            };
            let mut cursor = RunCursor::new(&sir, &measure, &request, None);
            let outcome = loop {
                if let CursorState::Finished(outcome) = cursor.advance(1 << 20) {
                    break outcome;
                }
            };
            assert_eq!(outcome.reducers, row.outcome.reducers, "{name}: run {run_id} replays");
        }
    }
    let files = ResultSet::open_dir(&scratch.path().join("pse"), usize::MAX).expect("the folder reads back");
    let picked: Vec<(String, Vec<u8>)> = [MANIFEST_FILE, RUNS_FILE, EVALUATIONS_FILE, BATCHES_FILE, ARCHIVE_FILE]
        .iter()
        .map(|file| {
            let bytes = std::fs::read(scratch.path().join("pse").join(file)).expect("the file is written");
            (format!("picked {file}"), bytes)
        })
        .collect();
    let from_files = ResultSet::from_files(picked, usize::MAX).expect("the picked files read");
    assert_eq!(
        from_files.search_history().expect("the tables read"),
        files.search_history().expect("the tables read"),
        "a picked file is known by its header"
    );
}

#[test]
fn a_search_folder_with_an_axis_of_no_cells_is_refused() {
    let scratch = ScratchDir::new("search-no-cells");
    let spec = search_spec(pattern(), 6);
    search(&entry("sir", None), None, &spec, scratch.path(), lane_count(1));
    let manifest_path = scratch.path().join(MANIFEST_FILE);
    let manifest = std::fs::read_to_string(&manifest_path).expect("the manifest is written");
    assert!(manifest.contains("\"cells\": 8"), "the manifest records the axis cells");
    std::fs::write(&manifest_path, manifest.replacen("\"cells\": 8", "\"cells\": 0", 1)).expect("the manifest writes");
    let opened = ResultSet::open_dir(scratch.path(), usize::MAX);
    assert!(matches!(opened, Err(ResultSetError::Search(_))), "{opened:?}");
}
