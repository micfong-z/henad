//! Fowler-Noll-Vo (FNV-1a) 64-bit hashes naming a model's declarations, a plan and a run.
//!
//! Unlike [`std::hash::DefaultHasher`], these hashes never change between Rust releases or platforms. A hash in an
//! output file can be compared with one a later build computes.

use crate::explore::factor::{FactorSpec, FactorTarget, LevelSpec};
use crate::explore::plan::{Config, ModelSchema, PlannedBlock};
use crate::explore::search::pse::PatternAxis;
use crate::explore::search::{SearchAlgorithm, SearchSpec};
use crate::explore::spec::{ActionSpec, MeasureSettings, RunSettings, SeedSettings};
use crate::params::{ParamKind, ParamValue};

const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a hasher over 64 bits.
///
/// Integers are written little-endian, and text is written after its length, so two fields never run together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a64 {
    state: u64,
}

impl Fnv1a64 {
    pub const fn new() -> Self {
        Self { state: OFFSET_BASIS }
    }

    pub const fn write(&mut self, bytes: &[u8]) {
        let mut index = 0;
        while index < bytes.len() {
            self.state ^= bytes[index] as u64;
            self.state = self.state.wrapping_mul(PRIME);
            index += 1;
        }
    }

    pub const fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    pub const fn write_str(&mut self, text: &str) {
        self.write_u64(text.len() as u64);
        self.write(text.as_bytes());
    }

    /// Writes the bits of `value`, so two values hash alike only when they are bit for bit the same.
    pub fn write_f64(&mut self, value: f64) {
        self.write_u64(value.to_bits());
    }

    fn write_value(&mut self, value: &ParamValue) {
        let (tag, bits) = match *value {
            ParamValue::F32(number) => (0, u64::from(number.to_bits())),
            ParamValue::U32(number) => (1, u64::from(number)),
            ParamValue::Bool(flag) => (2, u64::from(flag)),
            ParamValue::Choice(index) => (3, index as u64),
        };
        self.write(&[tag]);
        self.write_u64(bits);
    }

    pub const fn finish(&self) -> u64 {
        self.state
    }
}

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the FNV-1a hash of `bytes`.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write(bytes);
    hasher.finish()
}

/// Returns the hash of a model's parameter ids, kinds, bounds, defaults and options, its stat labels and its action
/// ids.
///
/// Labels shown in the UI, colours, slider steps and display formats are left out.
pub fn schema_hash(schema: &ModelSchema<'_>) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write_str("henad.schema.1");
    hasher.write_str(schema.id);
    hasher.write_u64(schema.params.len() as u64);
    for param in schema.params {
        hasher.write_str(param.id);
        match param.kind {
            ParamKind::F32 { min, max, default, .. } => {
                hasher.write(&[0]);
                for number in [min, max, default] {
                    hasher.write_u64(u64::from(number.to_bits()));
                }
            }
            ParamKind::U32 { min, max, default } => {
                hasher.write(&[1]);
                for number in [min, max, default] {
                    hasher.write_u64(u64::from(number));
                }
            }
            ParamKind::Bool { default } => {
                hasher.write(&[2]);
                hasher.write_u64(u64::from(default));
            }
            ParamKind::Choice { options, default } => {
                hasher.write(&[3]);
                hasher.write_u64(options.len() as u64);
                for option in options {
                    hasher.write_str(option);
                }
                hasher.write_u64(default as u64);
            }
        }
    }
    hasher.write_u64(schema.stats.len() as u64);
    for stat in schema.stats {
        hasher.write_str(stat.label);
    }
    hasher.write_u64(schema.actions.len() as u64);
    for action in schema.actions {
        hasher.write_str(action.id);
    }
    hasher.finish()
}

/// Returns the hash of every setting that shapes the results of one run, given its seed, parameter values and
/// action ticks.
///
/// The hash covers the model's [`schema_hash`], the warm-up and step counts, the stop condition, the sampling cadence,
/// the reducers and the ids of the actions. The timeout is left out, since a run that ends within it gives the same
/// results under any timeout.
pub fn results_fingerprint(
    schema_hash: u64,
    run: &RunSettings,
    measure: &MeasureSettings,
    actions: &[ActionSpec],
) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write_str("henad.results.2");
    hasher.write_u64(schema_hash);
    hasher.write_u64(run.warmup);
    hasher.write_u64(run.steps);
    match &run.stop {
        None => hasher.write(&[0]),
        Some(stop) => {
            hasher.write(&[1]);
            hasher.write_str(&stop.column);
            hasher.write_str(stop.comparison.comparator.as_str());
            hasher.write_u64(stop.comparison.threshold.to_bits());
            hasher.write_u64(stop.min_tick);
        }
    }
    hasher.write_u64(measure.stats_every);
    hasher.write_u64(measure.series_every);
    hasher.write(&[u8::from(measure.default_reducers)]);
    hasher.write_u64(measure.reducers.len() as u64);
    for reducer in &measure.reducers {
        hasher.write_str(&reducer.column);
        hasher.write_str(&reducer.kind.to_string());
    }
    hasher.write_u64(actions.len() as u64);
    for action in actions {
        hasher.write_str(&action.id);
    }
    hasher.finish()
}

/// Returns the key naming the results of the run with parameter values `params`, action ticks `action_ticks` and
/// seed `seed`.
pub fn run_key(results_fingerprint: u64, params: &[ParamValue], action_ticks: &[u64], seed: u64) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write_str("henad.run.2");
    hasher.write_u64(results_fingerprint);
    hasher.write_u64(params.len() as u64);
    for value in params {
        hasher.write_value(value);
    }
    hasher.write_u64(action_ticks.len() as u64);
    for &tick in action_ticks {
        hasher.write_u64(tick);
    }
    hasher.write_u64(seed);
    hasher.finish()
}

/// Returns the hash of every setting that fixes a plan's configs and their seeds.
///
/// The replicate count is left out. No run's seed depends on it, and a plan with more replicates keeps the hash.
/// The timeout is left out as well.
pub fn plan_hash(
    results_fingerprint: u64,
    seeds: &SeedSettings,
    actions: &[ActionSpec],
    blocks: &[PlannedBlock],
    configs: &[Config],
) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write_str("henad.plan.2");
    hasher.write_u64(results_fingerprint);
    hasher.write_u64(seeds.root);
    hasher.write_str(seeds.scheme.as_str());
    hasher.write_u64(actions.len() as u64);
    for action in actions {
        hasher.write_str(&action.name);
    }
    hasher.write_u64(blocks.len() as u64);
    for block in blocks {
        hasher.write_str(block.design.as_str());
        match block.design_seed {
            None => hasher.write(&[0]),
            Some(seed) => {
                hasher.write(&[1]);
                hasher.write_u64(seed);
            }
        }
    }
    hasher.write_u64(configs.len() as u64);
    for config in configs {
        hasher.write_u64(config.block as u64);
        hasher.write_u64(config.params.len() as u64);
        for value in &config.params {
            hasher.write_value(value);
        }
        hasher.write_u64(config.action_ticks.len() as u64);
        for &tick in &config.action_ticks {
            hasher.write_u64(tick);
        }
    }
    hasher.finish()
}

/// Returns the hash of every setting that fixes a search's trajectory, over the plan hash of its fixed values.
///
/// The hash covers `plan_hash`, the replicate count, the budget, the batch size, the objective, the space and the
/// settings of the algorithm. Note that the replicate count is covered, though [`plan_hash`] leaves it out.
pub fn search_hash(plan_hash: u64, replicates: u64, search: &SearchSpec) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write_str("henad.search.1");
    hasher.write_u64(plan_hash);
    hasher.write_u64(replicates);
    hasher.write_str(search.algorithm.as_str());
    hasher.write_u64(search.max_evaluations);
    hasher.write_u64(search.batch_size as u64);
    match &search.objective {
        None => hasher.write(&[0]),
        Some(objective) => {
            hasher.write(&[1]);
            hasher.write_str(&objective.column);
            hasher.write_str(objective.goal.as_str());
            hasher.write_str(objective.aggregate.as_str());
        }
    }
    hasher.write_u64(search.space.len() as u64);
    for factor in &search.space {
        write_factor(&mut hasher, factor);
    }
    match &search.algorithm {
        SearchAlgorithm::Random => {}
        SearchAlgorithm::HillClimb(settings) => {
            hasher.write_f64(settings.mutation_scale);
            hasher.write_u64(settings.patience);
            hasher.write(&[u8::from(settings.reevaluate)]);
        }
        SearchAlgorithm::Genetic(settings) => {
            for count in [settings.population, settings.elite_count, settings.tournament_size] {
                hasher.write_u64(count as u64);
            }
            for rate in [
                settings.crossover_rate,
                settings.mutation_rate,
                settings.mutation_scale,
                settings.reevaluate_fraction,
            ] {
                hasher.write_f64(rate);
            }
        }
        SearchAlgorithm::PatternSpaceExploration(settings) => {
            write_axis(&mut hasher, &settings.x_axis);
            write_axis(&mut hasher, &settings.y_axis);
            hasher.write_u64(settings.initial_samples);
            hasher.write_f64(settings.mutation_scale);
            hasher.write_str(settings.aggregate.as_str());
        }
    }
    hasher.finish()
}

/// Writes the target and levels of `factor`.
fn write_factor(hasher: &mut Fnv1a64, factor: &FactorSpec) {
    match &factor.target {
        FactorTarget::Param(id) => {
            hasher.write(&[0]);
            hasher.write_str(id);
        }
        FactorTarget::Action(name) => {
            hasher.write(&[1]);
            hasher.write_str(name);
        }
    }
    match &factor.levels {
        LevelSpec::Values(values) => {
            hasher.write(&[0]);
            hasher.write_u64(values.len() as u64);
            for value in values {
                hasher.write_str(value);
            }
        }
        &LevelSpec::Range { min, max, step } => {
            hasher.write(&[1]);
            hasher.write_f64(min);
            hasher.write_f64(max);
            match step {
                None => hasher.write(&[0]),
                Some(step) => {
                    hasher.write(&[1]);
                    hasher.write_f64(step);
                }
            }
        }
        LevelSpec::All => hasher.write(&[2]),
    }
}

/// Writes the column, bounds and cells of `axis`.
///
/// A bound left out is written as `NaN`, which no bound the check takes can be.
fn write_axis(hasher: &mut Fnv1a64, axis: &PatternAxis) {
    hasher.write_str(&axis.column);
    hasher.write_f64(axis.min.unwrap_or(f64::NAN));
    hasher.write_f64(axis.max.unwrap_or(f64::NAN));
    hasher.write_u64(u64::from(axis.cells));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{fnv1a64, results_fingerprint, run_key, schema_hash, search_hash};
    use crate::action::ActionDescriptor;
    use crate::explore::design::DesignKind;
    use crate::explore::factor::{FactorSpec, LevelSpec};
    use crate::explore::plan::{ModelSchema, Plan, PlanError};
    use crate::explore::search::pse::{PatternAxis, PatternSpaceSettings};
    use crate::explore::search::{SearchAlgorithm, SearchSpec};
    use crate::explore::spec::{ActionSpec, BlockSpec, MeasureSettings, RunSettings, SweepSpec};
    use crate::explore::stop::StopSpec;
    use crate::helpers::{f32_param, u32_param};
    use crate::params::{ParamDescriptor, ParamValue};
    use crate::view::StatDescriptor;

    const STATS: &[StatDescriptor] = &[StatDescriptor::new("Infected", [0, 0, 0, 255])];
    const ACTIONS: &[ActionDescriptor] = &[ActionDescriptor::new("seed_outbreak", "Seed outbreak")];

    fn params() -> Vec<ParamDescriptor> {
        vec![
            f32_param("infection_rate", "Infection rate", 0.2, 0.0, 1.0, Some(0.01)),
            u32_param("grid_width", "Grid width", 64, 1, 4096),
        ]
    }

    fn schema(params: &[ParamDescriptor]) -> ModelSchema<'_> {
        ModelSchema {
            id: "sir",
            params,
            stats: STATS,
            actions: ACTIONS,
        }
    }

    fn plan(spec: &SweepSpec) -> Result<Plan, PlanError> {
        spec.plan(&schema(&params()))
    }

    fn sweep() -> SweepSpec {
        let mut spec = SweepSpec::new("sir");
        spec.run.replicates = 2;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Range {
                    min: 0.1,
                    max: 0.3,
                    step: Some(0.1),
                },
            )],
            design_seed: None,
        }];
        spec
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn the_schema_hash_changes_with_a_bound() {
        let params = params();
        let original = schema_hash(&schema(&params));
        assert_eq!(
            original,
            schema_hash(&schema(&params)),
            "the same schema hashes the same"
        );

        let mut wider = params.clone();
        wider[1] = u32_param("grid_width", "Grid width", 64, 1, 8192);
        assert_ne!(original, schema_hash(&schema(&wider)));

        let mut relabeled = params.clone();
        relabeled[1] = u32_param("grid_width", "Width of the grid", 64, 1, 4096);
        assert_eq!(original, schema_hash(&schema(&relabeled)), "a label is left out");

        let without_actions = ModelSchema {
            actions: &[],
            ..schema(&params)
        };
        assert_ne!(original, schema_hash(&without_actions));
    }

    #[test]
    fn the_plan_hash_ignores_the_replicate_count() {
        let two = plan(&sweep()).expect("a valid spec");
        let mut spec = sweep();
        spec.run.replicates = 5;
        let five = plan(&spec).expect("a valid spec");
        assert_eq!(two.plan_hash(), five.plan_hash());
        assert_eq!(two.results_fingerprint(), five.results_fingerprint());
        for run in two.runs() {
            let extended = five
                .runs()
                .find(|other| other.config_id == run.config_id && other.rep == run.rep)
                .expect("five replicates include the first two");
            assert_eq!(run.seed, extended.seed);
            assert_eq!(two.run_key(&run), five.run_key(&extended));
        }

        let mut longer = sweep();
        longer.run.steps += 1;
        assert_ne!(plan(&longer).expect("a valid spec").plan_hash(), two.plan_hash());
        let mut reseeded = sweep();
        reseeded.seeds.root = 1;
        assert_ne!(plan(&reseeded).expect("a valid spec").plan_hash(), two.plan_hash());
        let mut fixed = sweep();
        fixed.fixed = vec![("grid_width".to_owned(), "32".to_owned())];
        assert_ne!(plan(&fixed).expect("a valid spec").plan_hash(), two.plan_hash());
    }

    #[test]
    fn a_run_key_names_the_values_the_ticks_and_the_seed() {
        let fingerprint = results_fingerprint(7, &RunSettings::default(), &MeasureSettings::default(), &[]);
        let params = [ParamValue::F32(0.1), ParamValue::U32(64)];
        let key = run_key(fingerprint, &params, &[], 3);
        assert_eq!(key, run_key(fingerprint, &params, &[], 3));
        assert_ne!(key, run_key(fingerprint, &params, &[], 4));
        assert_ne!(
            key,
            run_key(fingerprint, &[ParamValue::F32(0.2), ParamValue::U32(64)], &[], 3)
        );
        assert_ne!(key, run_key(fingerprint + 1, &params, &[], 3));
        assert_ne!(
            run_key(fingerprint, &params, &[10], 3),
            run_key(fingerprint, &params, &[20], 3)
        );
    }

    #[test]
    fn the_plan_hash_ignores_the_timeout() {
        let original = plan(&sweep()).expect("a valid spec");
        let mut timed = sweep();
        timed.run.timeout = Some(Duration::from_secs(600));
        let timed = plan(&timed).expect("a valid spec");
        assert_eq!(original.plan_hash(), timed.plan_hash());
        assert_eq!(original.results_fingerprint(), timed.results_fingerprint());
        let run = original.run(1).expect("run 1 exists");
        assert_eq!(original.run_key(&run), timed.run_key(&run));
    }

    #[test]
    fn the_plan_hash_covers_the_stop_the_actions_and_the_design_seed() {
        let original = plan(&sweep()).expect("a valid spec").plan_hash();
        let mut stopping = sweep();
        stopping.run.stop = Some(StopSpec::parse("Infected <= 0", 0).expect("a well-formed condition"));
        assert_ne!(plan(&stopping).expect("a valid spec").plan_hash(), original);

        let mut acting = sweep();
        acting.actions = vec![ActionSpec::new("seed_outbreak", 10)];
        let acting_hash = plan(&acting).expect("a valid spec").plan_hash();
        assert_ne!(acting_hash, original);
        acting.actions[0].tick = 20;
        assert_ne!(plan(&acting).expect("a valid spec").plan_hash(), acting_hash);
        acting.actions[0].tick = 10;
        acting.actions[0].name = "wave".to_owned();
        assert_ne!(
            plan(&acting).expect("a valid spec").plan_hash(),
            acting_hash,
            "a name heads a column"
        );

        let sampled = |design_seed| {
            let mut spec = sweep();
            spec.blocks[0].design = DesignKind::Random { samples: 4 };
            spec.blocks[0].factors[0].levels = LevelSpec::Range {
                min: 0.1,
                max: 0.3,
                step: None,
            };
            spec.blocks[0].design_seed = design_seed;
            plan(&spec).expect("a valid spec").plan_hash()
        };
        assert_eq!(sampled(Some(5)), sampled(Some(5)));
        assert_ne!(sampled(Some(5)), sampled(Some(6)));
    }

    /// Returns a Pattern Space Exploration over axes `x_axis` and `y_axis`.
    fn pattern_search(x_axis: PatternAxis, y_axis: PatternAxis) -> SearchSpec {
        SearchSpec {
            algorithm: SearchAlgorithm::PatternSpaceExploration(PatternSpaceSettings {
                initial_samples: 256,
                ..PatternSpaceSettings::new(x_axis, y_axis)
            }),
            max_evaluations: 1200,
            batch_size: 64,
            objective: None,
            space: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Range {
                    min: 0.05,
                    max: 0.9,
                    step: None,
                },
            )],
        }
    }

    #[test]
    fn an_automatic_range_hashes_apart_from_every_bound() {
        let bounded = pattern_search(
            PatternAxis::bounded("Infected:max", 0.0, 4096.0, 32),
            PatternAxis::bounded("Infected:argmax", 0.0, 50.0, 20),
        );
        // Pinned. A search folder whose axes have both bounds resumes only while this hash holds.
        assert_eq!(search_hash(0x1234, 2, &bounded), 0x1512_884c_c79f_9d8e);
        let automatic = pattern_search(
            PatternAxis::automatic("Infected:max", 32),
            PatternAxis::bounded("Infected:argmax", 0.0, 50.0, 20),
        );
        assert_ne!(search_hash(0x1234, 2, &automatic), search_hash(0x1234, 2, &bounded));
    }
}
