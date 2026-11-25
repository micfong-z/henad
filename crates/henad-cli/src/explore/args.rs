//! Flags of a sweep and the one-block sweep they describe.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};

use henad_core::action::{ScheduleError, parse_entry};
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, FactorTarget, LevelSpec};
use henad_core::explore::fingerprint::fnv1a64;
use henad_core::explore::plan::Shard;
use henad_core::explore::reducer::ReducerSpec;
use henad_core::explore::seed::SeedScheme;
use henad_core::explore::spec::{
    ACTION_COLUMN_PREFIX, ActionSpec, BlockSpec, MeasureSettings, RunSettings, SeedSettings, SweepSpec,
};
use henad_core::explore::stop::StopSpec;
use henad_explore::exec::Concurrency;
use henad_explore::spec_file::DesignTableFile;
use henad_explore::sweep::SpecSource;

use crate::Args;

/// Flags that make a sweep, or merge the shards of one.
///
/// `--out`, `--spec` and `--dry-run` each request a sweep, and every other flag apart from `--merge` needs one of
/// them.
#[derive(clap::Args, Debug, Clone, Default, PartialEq)]
#[command(next_help_heading = "Sweeps")]
pub struct ExploreArgs {
    /// Run a sweep and write its results to this directory. The directory must hold no results unless `--resume` is
    /// given. With `--merge`, write the merged shards to this directory.
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,

    /// Read the sweep from a TOML spec file. The file names the model and every setting that changes a result. With a
    /// `[search]` table in the file, run a search instead.
    #[arg(
        long,
        value_name = "FILE",
        requires = "spec_use",
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
    #[arg(long, value_name = "ID=LEVELS", value_parser = parse_vary, requires = "explore")]
    pub vary: Vec<FactorSpec>,

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

    /// Keep a series row every N ticks, a multiple of `--stats-every`. Set to 0 to keep no series. Defaults to
    /// `--stats-every`.
    #[arg(long, value_name = "N", requires = "explore")]
    pub series_every: Option<u64>,

    /// End a run at the first sample where a condition holds, e.g. `'Infected <= 0'`. The comparator is one of
    /// `<`, `<=`, `>`, `>=`, `==` and `!=`.
    #[arg(long, value_name = "CONDITION", value_parser = parse_stop, requires = "explore")]
    pub stop: Option<StopSpec>,

    /// Add a reducer over a stat column, e.g. `Infected:max`. Kinds are final, min, max, mean, argmax, argmin,
    /// `first<=10` for the first tick where a comparison holds, and `mean@200..600` for the mean from tick 200 to
    /// tick 600. Repeatable.
    #[arg(long, value_name = "COLUMN:KIND", value_parser = parse_reduce, requires = "explore")]
    pub reduce: Vec<ReducerSpec>,

    /// Remove the default final, min, max and mean reducers from every stat column.
    #[arg(long, requires = "explore")]
    pub no_default_reducers: bool,

    /// End a run after this many seconds of wall-clock time and record it as `timed_out`. With `--resume`, it will run
    /// again. For concurrent GPU runs, only each run's share of device time counts.
    #[arg(long, value_name = "SECONDS", value_parser = parse_timeout, requires = "explore")]
    pub timeout: Option<Duration>,

    /// Number of concurrent runs, or `auto`. Defaults to the spec's `[execution]` value, else `auto`.
    #[arg(long, value_name = "N|auto", requires = "explore")]
    pub concurrent: Option<Concurrency>,

    /// Combined memory limit in bytes for concurrent runs. Defaults to the spec's `[execution]` value, else no
    /// limit.
    #[arg(long, value_name = "BYTES", requires = "explore")]
    pub memory: Option<u64>,

    /// Combined GPU memory limit in bytes for concurrent GPU runs. Defaults to the spec's `[execution]` value, else
    /// the device's largest buffer size.
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

    /// Merge a sweep's shard directories into the `--out` directory. Takes no model.
    #[arg(
        long,
        value_name = "DIR",
        num_args = 1..,
        requires = "out",
        conflicts_with_all = [
            "model", "model_file", "list", "info", "params", "set", "act", "steps", "warmup", "reps", "seed",
            "stats_every", "spec", "dry_run", "vary", "zip", "sample", "design_seed", "design", "independent_seeds",
            "series_every", "stop", "reduce", "no_default_reducers", "timeout", "concurrent", "memory", "gpu_memory",
            "shard", "resume", "retry_failed",
        ]
    )]
    pub merge: Vec<PathBuf>,

    /// Print the sweep's plan and write nothing. The first config that builds without errors and the last config are
    /// built as a check.
    #[arg(long)]
    pub dry_run: bool,
}

impl ExploreArgs {
    /// Returns whether the flags request a sweep.
    pub fn is_sweep(&self) -> bool {
        self.out.is_some() || self.spec.is_some() || self.dry_run
    }
}

/// Returns the sweep the flags describe, one block over every `--vary` or over the `--design` table.
///
/// # Errors
///
/// Returns an error for an `--act` that is not of the form `ID@TICK`, a `--vary` over the tick of an action that no
/// `--act` adds, or a `--design` table that cannot be read.
pub fn spec_from_flags(args: &Args, model: &str) -> Result<SweepSpec> {
    let mut spec = SweepSpec::new(model);
    spec.fixed.clone_from(&args.set);
    spec.run = RunSettings {
        steps: args.steps,
        warmup: args.warmup,
        replicates: args.reps,
        stop: args.explore.stop.clone(),
        timeout: args.explore.timeout,
    };
    spec.measure = MeasureSettings {
        stats_every: args.stats_every,
        series_every: args.explore.series_every.unwrap_or(args.stats_every),
        default_reducers: !args.explore.no_default_reducers,
        reducers: args.explore.reduce.clone(),
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
    for factor in &args.explore.vary {
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
    } else if !args.explore.vary.is_empty() {
        let design = match (&args.explore.sample, args.explore.zip) {
            (Some(sample), _) => sample.clone(),
            (None, true) => DesignKind::Zip,
            (None, false) => DesignKind::Factorial,
        };
        spec.blocks = vec![BlockSpec {
            design,
            factors: args.explore.vary.clone(),
            design_seed: args.explore.design_seed,
        }];
    }
    Ok(spec)
}

/// Returns the record of a sweep built from flags, listing the `--design` table it read.
pub(super) fn flags_source(args: &Args, spec: &SweepSpec) -> SpecSource {
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

/// Reads `--vary ID=LEVELS` into a factor, with `LEVELS` in the text form that [`LevelSpec::parse`] accepts.
///
/// An id of the form `action.NAME` makes a factor over the tick of the action named `NAME`. The model checks the id
/// and the levels when the sweep is planned. The parser then rejects a malformed flag.
fn parse_vary(raw: &str) -> Result<FactorSpec, String> {
    let (target, levels) = raw.split_once('=').ok_or("expected ID=LEVELS")?;
    let levels = LevelSpec::parse(levels).map_err(|error| error.to_string())?;
    Ok(match target.strip_prefix(ACTION_COLUMN_PREFIX) {
        Some(name) => FactorSpec::action(name, levels),
        None => FactorSpec::param(target, levels),
    })
}

/// Reads `--stop` as a condition.
///
/// The sweep checks the column once the model's columns are known.
fn parse_stop(raw: &str) -> Result<StopSpec, String> {
    StopSpec::parse(raw, 0).map_err(|error| error.to_string())
}

/// Reads `--reduce` as `COLUMN:KIND`.
///
/// The sweep checks the column once the model's columns are known.
fn parse_reduce(raw: &str) -> Result<ReducerSpec, String> {
    raw.parse::<ReducerSpec>().map_err(|error| error.to_string())
}

/// Returns the actions `--act` adds to every run of a sweep.
///
/// An action is named by its id, or by the first of `ID_2`, `ID_3` and so on that no earlier action uses. The
/// second `--act` of an id is then `ID_2` and the third `ID_3`, unless an earlier entry uses that name. The model
/// checks the ids when the sweep is planned.
///
/// # Errors
///
/// Returns [`ScheduleError`] for an entry that is not of the form `ID@TICK`.
pub fn fixed_actions(raw: &[String]) -> Result<Vec<ActionSpec>, ScheduleError> {
    let mut actions: Vec<ActionSpec> = Vec::with_capacity(raw.len());
    for entry in raw {
        let (id, tick) = parse_entry(entry)?;
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

/// Reads `--timeout` as a number of seconds from 0 to `u64::MAX`.
fn parse_timeout(raw: &str) -> Result<Duration, String> {
    raw.parse::<f64>()
        .ok()
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .ok_or_else(|| {
            format!(
                "expected a finite number of seconds from 0 to {}, got '{raw}'",
                u64::MAX
            )
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use clap::Parser as _;
    use clap::error::ErrorKind;
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorTarget, LevelSpec};
    use henad_core::explore::plan::Shard;
    use henad_core::explore::seed::SeedScheme;
    use henad_core::explore::spec::ActionSpec;
    use henad_core::explore::stop::StopSpec;
    use henad_models::example_models;

    use super::{fixed_actions, parse_vary, spec_from_flags};
    use crate::single::SingleRun;
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
            Mode::Single(SingleRun::Benchmark),
            "outside a sweep --act keeps its meaning"
        );
    }

    /// Checks that a repeated id takes the first free name. Numbered by its count alone, it could share a name with an
    /// explicit `ID_2`, and the plan would reject the sweep.
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

    /// Checks that an entry splits at its last `@`. If it split at the first `@`, the rest of an id that contains `@`
    /// would be read as the tick.
    #[test]
    fn an_action_id_can_hold_an_at_sign() {
        let actions = fixed_actions(&["spawn@centre@100".to_owned()]).expect("the entry reads");
        assert_eq!(actions, [ActionSpec::new("spawn@centre", 100)]);
        let args = Args::try_parse_from(["henad-cli", "sir", "--act", "spawn@centre@100"]);
        assert!(args.is_ok(), "the flag reads");
    }

    /// Checks that `--info` beside a sweep runs the sweep, and that `--list` rejects a sweep. Otherwise either flag
    /// would print and exit, and the requested sweep would never run.
    #[test]
    fn info_and_list_do_not_drop_a_sweep() {
        let assert_mode = |line: &[&str], expected: Mode<'_>| {
            let args = Args::try_parse_from(line).unwrap_or_else(|error| panic!("{line:?} parses: {error}"));
            assert_eq!(Mode::of(&args), expected, "{line:?}");
        };
        assert_mode(&["henad-cli", "--info"], Mode::InfoOnly);
        // The sweep then asks for a model.
        assert_mode(
            &["henad-cli", "--info", "--out", "d", "--vary", "infection_rate=0.1,0.2"],
            Mode::Explore,
        );
        assert_mode(&["henad-cli", "--info", "--dry-run"], Mode::Explore);
        assert_mode(
            &["henad-cli", "--info", "--spec", "s.toml", "--out", "d"],
            Mode::Explore,
        );
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
        for bad in ["-1", "soon", "NaN", "inf", "1e30"] {
            let line = ["henad-cli", "sir", "--out", "d", &format!("--timeout={bad}")];
            assert_eq!(parse(&line), Err(ErrorKind::ValueValidation), "--timeout {bad}");
            let message = Args::try_parse_from(line).err().map(|error| error.to_string());
            assert!(
                message
                    .as_deref()
                    .is_some_and(|message| message.contains("finite number of seconds")),
                "--timeout {bad}: {message:?}"
            );
        }
    }

    /// Checks that the parser rejects a malformed `--set`, `--act`, `--vary`, `--stop` or `--reduce`. If the value
    /// were read after parsing, the command would exit 1, the status of a failed sweep, instead of 2.
    #[test]
    fn malformed_flag_values_are_refused_by_the_parser() {
        let refused = [
            ("--set", "grid_width"),
            ("--act", "seed_outbreak"),
            ("--act", "seed_outbreak@soon"),
            ("--act", "seed_outbreak@-1"),
            ("--vary", "infection_rate"),
            ("--vary", "infection_rate=0.1:x:0.1"),
            ("--stop", "Infected"),
            ("--reduce", "Infected:median"),
            ("--reduce", "Infected"),
        ];
        for (flag, value) in refused {
            let line = ["henad-cli", "sir", "--out", "d", flag, value];
            assert_eq!(parse(&line), Err(ErrorKind::ValueValidation), "{flag} {value}");
        }
        let accepted = [
            ("--set", "grid_width=64"),
            ("--act", "seed_outbreak@40"),
            ("--vary", "infection_rate=0.1:0.5:0.1"),
            ("--stop", "Infected <= 0"),
            ("--reduce", "Infected:first<=10"),
        ];
        for (flag, value) in accepted {
            let line = ["henad-cli", "sir", "--out", "d", flag, value];
            assert_eq!(parse(&line), Ok(()), "{flag} {value}");
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

    /// Returns the kind of error that `line` produces, or `Ok` when it parses.
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
        let missing: [&[&str]; 13] = [
            // A spec needs `--out`, `--dry-run` or `--params`. The parser rejects a line with none of these flags,
            // before a device is acquired or the file read.
            &["henad-cli", "--spec", "s.toml"],
            &["henad-cli", "--info", "--spec", "s.toml"],
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
}
