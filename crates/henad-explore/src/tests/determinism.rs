//! Checks that a sweep's results stay the same at any lane count, lane width or sampling cadence.

use std::num::NonZeroUsize;
use std::sync::Arc;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::seed::{SeedScheme, run_seed};
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_core::explore::stop::StopSpec;
use henad_core::export::csv::parse_records;
use henad_core::export::state::{point_rows, write_edges, write_grid, write_points};
use henad_core::model::SimState;

use crate::exec::Concurrency;
use crate::output::OutputWriter;
use crate::output::runs_csv::RunsWriter;
use crate::output::series_csv::SeriesWriter;
use crate::tests::support::{
    Collected, OutputTables, ScratchDir, entry, lanes, planned, run_plan, sweep, without_timing,
};

fn values(raw: &[&str]) -> LevelSpec {
    LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
}

fn fixed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect()
}

fn lane_count(count: usize) -> Concurrency {
    Concurrency::Fixed(NonZeroUsize::new(count).expect("a test lane count is above 0"))
}

#[test]
fn a_sweep_writes_the_same_files_at_any_concurrency() {
    let sir = entry("sir", None);
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 40;
    spec.run.replicates = 3;
    spec.measure.stats_every = 4;
    spec.measure.series_every = 8;
    spec.seeds.root = 9;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![
            FactorSpec::param("infection_rate", values(&["0.2", "0.4", "0.6"])),
            FactorSpec::param(
                "recovery_rate",
                LevelSpec::Range {
                    min: 0.05,
                    max: 0.1,
                    step: Some(0.05),
                },
            ),
        ],
        design_seed: None,
    }];

    let scratch = ScratchDir::new("any-concurrency");
    let tables: Vec<OutputTables> = [1, 3, 4]
        .into_iter()
        .map(|count| {
            let output_dir = scratch.path().join(format!("lanes-{count}"));
            let report = sweep(&sir, None, &spec, &output_dir, lane_count(count));
            assert_eq!(report.outline.layout.cpu_lanes, count);
            assert_eq!((report.counts.rows, report.counts.ok), (18, 18));
            OutputTables::read(&output_dir)
        })
        .collect();
    let first = &tables[0];
    assert_eq!(first.runs.len(), 1 + 18, "a header and 3 x 2 configs of 3 replicates");
    assert_eq!(first.summary.len(), 1 + 6);
    assert_eq!(
        first.series.len(),
        1 + 18 * 6,
        "ticks 0, 8, 16, 24, 32 and 40 of every run"
    );
    assert_eq!(
        first.run_column("recovery_rate")[..6],
        ["0.05", "0.05", "0.05", "0.1", "0.1", "0.1"],
        "the last factor varies fastest"
    );
    for (other, count) in tables[1..].iter().zip([3, 4]) {
        assert_eq!(other, first, "{count} lanes");
    }
}

#[test]
fn a_sampled_design_sweep_writes_the_same_files_at_any_concurrency() {
    let sir = entry("sir", None);
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "24"), ("grid_height", "24")]);
    spec.run.steps = 60;
    spec.run.replicates = 2;
    spec.run.stop = Some(StopSpec::parse("Infected <= 0", 10).expect("a valid condition"));
    spec.measure.stats_every = 5;
    spec.measure.series_every = 10;
    spec.measure.reducers = ["Infected:argmax", "Infected:first<=5", "Recovered:mean@10..40"]
        .map(|raw| raw.parse().expect("a valid reducer"))
        .to_vec();
    spec.seeds.root = 31;
    spec.actions = vec![ActionSpec {
        name: "wave".to_owned(),
        ..ActionSpec::new("seed_outbreak", 20)
    }];
    let range = |min, max| LevelSpec::Range { min, max, step: None };
    spec.blocks = vec![BlockSpec {
        design: DesignKind::LatinHypercube { samples: 6 },
        factors: vec![
            FactorSpec::param("infection_rate", range(0.05, 0.9)),
            FactorSpec::param("recovery_rate", range(0.05, 0.4)),
            FactorSpec::action("wave", range(5.0, 50.0)),
        ],
        design_seed: None,
    }];

    let scratch = ScratchDir::new("sampled-any-concurrency");
    let tables: Vec<OutputTables> = [1, 3]
        .into_iter()
        .map(|count| {
            let output_dir = scratch.path().join(format!("lanes-{count}"));
            let report = sweep(&sir, None, &spec, &output_dir, lane_count(count));
            assert_eq!(report.outline.layout.cpu_lanes, count);
            assert_eq!(report.counts.rows, 12);
            OutputTables::read(&output_dir)
        })
        .collect();
    // The wave's range holds 46 whole ticks. Stratum `s` of the 6 takes tick 5 + s * 46 / 6, and each config appears
    // once per replicate.
    let mut waves: Vec<u64> = tables[0]
        .run_column("action.wave")
        .iter()
        .map(|tick| tick.parse().expect("a tick"))
        .collect();
    waves.sort_unstable();
    let strata: Vec<u64> = (0..6).flat_map(|stratum| [5 + stratum * 46 / 6; 2]).collect();
    assert_eq!(waves, strata, "one config in each stratum of the wave's ticks");
    assert_eq!(tables[0].summary.len(), 1 + 6);
    assert_eq!(tables[0], tables[1]);
}

#[test]
fn ants_results_do_not_depend_on_lane_width() {
    let ants = entry("ants", None);
    let mut spec = SweepSpec::new("ants");
    spec.fixed = fixed(&[("num_agents", "9000"), ("world_width", "96"), ("world_height", "96")]);
    spec.run.steps = 30;
    spec.run.replicates = 2;
    spec.measure.stats_every = 5;
    spec.measure.series_every = 10;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param("momentum", values(&["0.8", "0.4"]))],
        design_seed: None,
    }];
    let (plan, measure) = planned(&ants, None, &spec);
    let shared_plan = Arc::new(plan.clone());

    let written: Vec<(Vec<Vec<String>>, String)> = [lanes(1, 4), lanes(4, 1)]
        .into_iter()
        .map(|layout| {
            let runs = RunsWriter::new(
                Vec::new(),
                &ants.param_descriptors,
                plan.actions(),
                measure.reducers().names(),
            )
            .expect("a vector takes every write");
            let series = SeriesWriter::new(Vec::new(), measure.columns()).expect("a vector takes every write");
            let mut writer = OutputWriter::new(Arc::clone(&shared_plan), runs, series);
            run_plan(&ants, None, &plan, &measure, layout, &mut writer);
            assert_eq!(writer.counts().ok, 4, "{layout:?}");
            let (runs, series) = writer.finish().expect("a vector flushes");
            let runs = String::from_utf8(runs).expect("the rows are UTF-8");
            let runs = without_timing(parse_records(&runs).expect("valid CSV"));
            (runs, String::from_utf8(series).expect("the rows are UTF-8"))
        })
        .collect();
    assert_eq!(written[0], written[1]);
}

#[test]
fn common_random_numbers_share_seeds_across_configs() {
    let sir = entry("sir", None);
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "16"), ("grid_height", "16")]);
    spec.run.steps = 12;
    spec.run.replicates = 3;
    spec.seeds.root = 21;
    // Configs 1 and 2 hold the same values, one from each block.
    spec.blocks = vec![
        BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param("infection_rate", values(&["0.2", "0.5"]))],
            design_seed: None,
        },
        BlockSpec {
            design: DesignKind::Zip,
            factors: vec![FactorSpec::param("infection_rate", values(&["0.5"]))],
            design_seed: None,
        },
    ];

    let scratch = ScratchDir::new("common-random-numbers");
    let common = sweep(&sir, None, &spec, &scratch.path().join("common"), lane_count(2));
    assert_eq!(common.counts.rows, 9);
    let common = OutputTables::read(&scratch.path().join("common"));
    let seeds = common.run_column("seed");
    let expected: Vec<String> = (0..3).map(|rep| run_seed(21, rep).to_string()).collect();
    for config in seeds.chunks(3) {
        assert_eq!(config, expected, "replicate r of every config shares a seed");
    }
    let keys = common.run_column("run_key");
    assert_ne!(keys[..3], keys[3..6], "configs 0 and 1 differ");
    assert_eq!(keys[3..6], keys[6..], "configs 1 and 2 are the same runs");
    let infected = common.run_column("Infected:max");
    assert_eq!(infected[3..6], infected[6..], "the same runs give the same results");

    spec.seeds.scheme = SeedScheme::Independent;
    sweep(&sir, None, &spec, &scratch.path().join("independent"), lane_count(2));
    let independent = OutputTables::read(&scratch.path().join("independent"));
    let seeds = independent.run_column("seed");
    assert_ne!(seeds[3..6], seeds[6..], "every run has a seed of its own");
    assert_ne!(seeds[..3], seeds[3..6]);
}

/// Checks that sampling a run every tick or every tenth tick leaves it on the same trajectory.
///
/// A sample calls `prepare_view` with the state borrowed mutably. Each case steps one model twice, once for each
/// cadence, and compares the rows at the ticks both sample and the state exported at the end.
mod sampling_cadence_does_not_change_the_trajectory {
    use super::{
        Collected, MeasurePlan, ModelEntry, ModelState, SimState, SweepSpec, entry, fixed, lanes, planned, point_rows,
        run_plan, write_edges, write_grid, write_points,
    };

    /// Final tick of every case. It is off the coarser cadence, so the final sample is a sample of its own.
    const STEPS: u64 = 25;

    /// Cadences each case compares.
    const CADENCES: [u64; 2] = [1, 10];

    /// Registered CPU models, one case each.
    const CASES: [&str; 6] = ["sir", "game_of_life", "boids", "ants", "virus_network", "team_assembly"];

    /// Rows sampled at the ticks every cadence samples, and the exported final state.
    #[derive(Debug, PartialEq, Eq)]
    struct Trajectory {
        /// Tick and value bits of each row.
        rows: Vec<(u64, Vec<u64>)>,
        state: String,
    }

    fn trajectory(model: &ModelEntry, fixed_values: &[(&str, &str)], stats_every: u64) -> Trajectory {
        let mut spec = SweepSpec::new(model.id().to_owned());
        spec.fixed = fixed(fixed_values);
        spec.run.steps = STEPS;
        spec.measure.stats_every = stats_every;
        spec.measure.series_every = stats_every;
        spec.seeds.root = 5;
        let (plan, measure) = planned(model, None, &spec);
        let mut collected = Collected::default();
        run_plan(
            model,
            None,
            &plan,
            &measure,
            lanes(1, rayon::current_num_threads()),
            &mut collected,
        );
        let outcome = &collected.0[0];
        assert_eq!(outcome.ticks, STEPS, "{:?}", outcome.note);
        let rows = outcome
            .series
            .rows()
            .filter(|&(tick, _)| tick == STEPS || tick % CADENCES[1] == 0)
            .map(|(tick, values)| (tick, values.iter().map(|value| value.to_bits()).collect()))
            .collect();

        let run = plan.run(0).expect("the plan has a run");
        let params = &plan.config(run.config_id).expect("the run's config").params;
        let Ok(ModelState::Cpu(mut state)) = model.build(params, Some(run.seed), None) else {
            panic!("{} builds on the CPU", model.id());
        };
        sample_along(&mut *state, &measure);
        Trajectory {
            rows,
            state: exported(&mut *state),
        }
    }

    /// Steps `state` to the end of `measure`, sampling it at every tick `measure` samples as a run does.
    fn sample_along(state: &mut dyn SimState, measure: &MeasurePlan) {
        for tick in measure.sample_ticks() {
            while state.tick() < tick {
                state.step();
            }
            state.prepare_view();
            assert!(!state.stats().is_empty());
        }
    }

    /// Returns the state as `henad-cli --export` writes it.
    fn exported(state: &mut dyn SimState) -> String {
        state.prepare_view();
        let mut out = Vec::new();
        if let Some(grid) = state.grid_view() {
            write_grid(&mut out, grid.width, grid.height, grid.cells).expect("a vector takes every write");
        }
        if let Some(points) = state.point_view() {
            write_points(&mut out, points.pos_x, points.pos_y, points.color).expect("a vector takes every write");
            if let Some(edges) = state.edge_view() {
                let rows = point_rows(points.pos_x, points.pos_y);
                write_edges(&mut out, edges.src, edges.dst, edges.color.unwrap_or(&[]), &rows)
                    .expect("a vector takes every write");
            }
        }
        assert!(!out.is_empty(), "the model has a view to export");
        String::from_utf8(out).expect("the export is UTF-8")
    }

    fn check(id: &str, fixed_values: &[(&str, &str)]) {
        let model = entry(id, None);
        let [every_tick, every_tenth] = CADENCES.map(|stats_every| trajectory(&model, fixed_values, stats_every));
        assert_eq!(every_tick.rows.len(), 4, "ticks 0, 10, 20 and {STEPS}");
        assert_eq!(every_tick, every_tenth, "{id}");
    }

    #[test]
    fn every_cpu_model_has_a_case() {
        let mut registered: Vec<String> = henad_models::registry::model_registry(None)
            .into_iter()
            .filter(|model| model.metadata().backend == henad_core::metadata::Backend::Cpu)
            .map(|model| model.id().to_owned())
            .collect();
        registered.sort_unstable();
        let mut cases = CASES.map(str::to_owned).to_vec();
        cases.sort_unstable();
        assert_eq!(cases, registered);
    }

    #[test]
    fn sir() {
        check("sir", &[("grid_width", "24"), ("grid_height", "24")]);
    }

    #[test]
    fn game_of_life() {
        check("game_of_life", &[("grid_width", "24"), ("grid_height", "24")]);
    }

    #[test]
    fn boids() {
        check(
            "boids",
            &[("num_agents", "300"), ("world_width", "200"), ("world_height", "200")],
        );
    }

    #[test]
    fn ants() {
        check(
            "ants",
            &[("num_agents", "300"), ("world_width", "64"), ("world_height", "64")],
        );
    }

    #[test]
    fn virus_network() {
        check(
            "virus_network",
            &[("num_agents", "200"), ("world_width", "200"), ("world_height", "200")],
        );
    }

    #[test]
    fn team_assembly() {
        check("team_assembly", &[("num_agents", "50")]);
    }
}
