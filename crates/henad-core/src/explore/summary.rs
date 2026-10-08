//! Statistics over the replicates of each config.
//!
//! The 95% confidence interval for a mean is `mean ± t * sd / sqrt(n)`, where `t` is the 97.5% quantile of
//! Student's t distribution with `n - 1` degrees of freedom.

use std::collections::BTreeMap;

use crate::explore::outcome::RunStatus;

/// Quantiles of Student's t distribution at 97.5% for 1 to 30 degrees of freedom.
const T_975: [f64; 30] = [
    12.706_204_736,
    4.302_652_730,
    3.182_446_305,
    2.776_445_105,
    2.570_581_836,
    2.446_911_851,
    2.364_624_252,
    2.306_004_135,
    2.262_157_163,
    2.228_138_852,
    2.200_985_160,
    2.178_812_830,
    2.160_368_656,
    2.144_786_688,
    2.131_449_546,
    2.119_905_299,
    2.109_815_578,
    2.100_922_040,
    2.093_024_054,
    2.085_963_447,
    2.079_613_845,
    2.073_873_068,
    2.068_657_610,
    2.063_898_562,
    2.059_538_553,
    2.055_529_439,
    2.051_830_516,
    2.048_407_142,
    2.045_229_642,
    2.042_272_456,
];

/// Quantile of the standard normal distribution at 97.5%.
const Z_975: f64 = 1.959_963_984_540_054;

/// Returns the 97.5% quantile of Student's t distribution with `degrees_of_freedom` degrees of freedom.
///
/// Up to 30 degrees of freedom the value comes from a table, and past it from the Cornish-Fisher expansion in
/// `1 / degrees_of_freedom`. Note that the result is infinity for 0 degrees of freedom.
pub fn student_t_975(degrees_of_freedom: u64) -> f64 {
    if degrees_of_freedom == 0 {
        return f64::INFINITY;
    }
    if let Some(&quantile) = usize::try_from(degrees_of_freedom - 1)
        .ok()
        .and_then(|index| T_975.get(index))
    {
        return quantile;
    }
    let z = Z_975;
    let z2 = z * z;
    let z3 = z2 * z;
    let z5 = z3 * z2;
    let z7 = z5 * z2;
    let z9 = z7 * z2;
    let g1 = (z3 + z) / 4.0;
    let g2 = (5.0 * z5 + 16.0 * z3 + 3.0 * z) / 96.0;
    let g3 = (3.0 * z7 + 19.0 * z5 + 17.0 * z3 - 15.0 * z) / 384.0;
    let g4 = (79.0 * z9 + 776.0 * z7 + 1482.0 * z5 - 1920.0 * z3 - 945.0 * z) / 92160.0;
    let inverse = 1.0 / degrees_of_freedom as f64;
    z + inverse * (g1 + inverse * (g2 + inverse * (g3 + inverse * g4)))
}

/// Count, mean and spread of a stream of values, updated one value at a time with Welford's method.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RunningMoments {
    count: u64,
    mean: f64,
    /// Sum of squared distances from the mean.
    squares: f64,
}

impl RunningMoments {
    /// Adds `value`. A value that is not finite is left out.
    pub fn push(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        self.squares += delta * (value - self.mean);
    }

    /// Number of finite values pushed.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Returns the mean, or `None` before any value.
    pub fn mean(&self) -> Option<f64> {
        (self.count > 0).then_some(self.mean)
    }

    /// Returns the sample standard deviation, with `n - 1` in the denominator, or `None` below two values.
    pub fn standard_deviation(&self) -> Option<f64> {
        (self.count > 1).then(|| (self.squares / (self.count - 1) as f64).sqrt())
    }

    /// Returns the count, mean, standard deviation and 95% confidence interval of the values so far.
    pub fn summary(&self) -> ReplicateSummary {
        let standard_deviation = self.standard_deviation();
        let ci95 = standard_deviation.map(|standard_deviation| {
            let half_width = student_t_975(self.count - 1) * standard_deviation / (self.count as f64).sqrt();
            (self.mean - half_width, self.mean + half_width)
        });
        ReplicateSummary {
            n: self.count,
            mean: self.mean(),
            standard_deviation,
            ci95,
        }
    }
}

/// Statistics of one output over the replicates of a config.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplicateSummary {
    /// Number of finite values.
    pub n: u64,
    /// Mean of the finite values, `None` when there are no finite values.
    pub mean: Option<f64>,
    /// Sample standard deviation, `None` below two values.
    pub standard_deviation: Option<f64>,
    /// Low and high ends of the 95% confidence interval for the mean, `None` below two values.
    pub ci95: Option<(f64, f64)>,
}

/// Returns the summary of the finite values in `values`.
pub fn summarize(values: impl IntoIterator<Item = f64>) -> ReplicateSummary {
    let mut moments = RunningMoments::default();
    for value in values {
        moments.push(value);
    }
    moments.summary()
}

/// Run counts and statistics of one config.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryRow {
    /// Id of the config.
    pub config_id: u64,
    /// Number of runs of the config, whatever their status.
    pub runs: u64,
    /// Number of runs whose status is [`RunStatus::Ok`].
    pub ok: u64,
    /// Number of runs that ended on a fault or a timeout, as [`RunStatus::is_failure`] decides.
    pub failed: u64,
    /// Tick each run ended on.
    pub ticks: ReplicateSummary,
    /// One summary per reducer.
    pub reducers: Vec<ReplicateSummary>,
}

/// Counts and running statistics of every config, fed one run at a time.
///
/// Tick and reducer statistics come from the runs that did not fail.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SummaryAccumulator {
    reducer_count: usize,
    configs: BTreeMap<u64, ConfigTally>,
}

#[derive(Debug, Clone, PartialEq)]
struct ConfigTally {
    runs: u64,
    ok: u64,
    failed: u64,
    ticks: RunningMoments,
    reducers: Vec<RunningMoments>,
}

impl SummaryAccumulator {
    /// Returns an accumulator for runs with `reducer_count` reducer values each.
    pub fn new(reducer_count: usize) -> Self {
        Self {
            reducer_count,
            configs: BTreeMap::new(),
        }
    }

    /// Adds a run of config `config_id` that ended with `status` at tick `ticks`, with the values of its reducers.
    ///
    /// `reducers` holds one value per reducer. Note that the order in which runs are added affects the statistics down
    /// to the last bit.
    pub fn push(&mut self, config_id: u64, status: RunStatus, ticks: u64, reducers: &[Option<f64>]) {
        let reducer_count = self.reducer_count;
        let tally = self.configs.entry(config_id).or_insert_with(|| ConfigTally {
            runs: 0,
            ok: 0,
            failed: 0,
            ticks: RunningMoments::default(),
            reducers: vec![RunningMoments::default(); reducer_count],
        });
        tally.runs += 1;
        if status == RunStatus::Ok {
            tally.ok += 1;
        }
        if status.is_failure() {
            tally.failed += 1;
            return;
        }
        tally.ticks.push(ticks as f64);
        for (moments, value) in tally.reducers.iter_mut().zip(reducers) {
            if let Some(value) = value {
                moments.push(*value);
            }
        }
    }

    /// Returns a row for each config with at least one run, in config order.
    pub fn rows(&self) -> impl Iterator<Item = SummaryRow> + '_ {
        self.configs.iter().map(|(&config_id, tally)| SummaryRow {
            config_id,
            runs: tally.runs,
            ok: tally.ok,
            failed: tally.failed,
            ticks: tally.ticks.summary(),
            reducers: tally.reducers.iter().map(RunningMoments::summary).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{RunningMoments, SummaryAccumulator, student_t_975, summarize};
    use crate::explore::outcome::RunStatus;

    fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
        (actual - expected).abs() <= tolerance
    }

    #[test]
    fn t_quantiles_match_the_table() {
        for (df, expected) in [(1, 12.706), (2, 4.303), (10, 2.228), (30, 2.042), (120, 1.980)] {
            let quantile = student_t_975(df);
            assert!(close(quantile, expected, 1e-3), "df {df}: {quantile}");
        }
        assert!(
            close(student_t_975(31), 2.039_513, 1e-6),
            "the expansion joins the table"
        );
        assert!(
            close(student_t_975(1_000_000), 1.959_966, 1e-6),
            "and tends to the normal quantile"
        );
        assert!(student_t_975(0).is_infinite());
    }

    #[test]
    fn a_replicate_summary_matches_hand_computed_statistics() {
        let summary = summarize([1.0, 2.0, 3.0, 4.0]);
        assert_eq!(summary.n, 4);
        assert_eq!(summary.mean, Some(2.5));
        let sd = summary.standard_deviation.expect("four values have a spread");
        assert!(close(sd, 1.2910, 1e-4), "sd {sd}");
        let (low, high) = summary.ci95.expect("four values have an interval");
        assert!(close(high - 2.5, 2.054, 1e-3), "half-width {}", high - 2.5);
        assert!(close(2.5 - low, high - 2.5, 1e-12), "the interval is symmetric");
    }

    #[test]
    fn a_single_replicate_has_no_spread() {
        let summary = summarize([7.0]);
        assert_eq!(summary.n, 1);
        assert_eq!(summary.mean, Some(7.0));
        assert_eq!(summary.standard_deviation, None);
        assert_eq!(summary.ci95, None);
        let empty = summarize([]);
        assert_eq!((empty.n, empty.mean), (0, None));
    }

    #[test]
    fn values_that_are_not_finite_are_left_out() {
        let mut moments = RunningMoments::default();
        for value in [1.0, f64::NAN, 3.0, f64::INFINITY] {
            moments.push(value);
        }
        assert_eq!(moments.count(), 2);
        assert_eq!(moments.mean(), Some(2.0));
    }

    #[test]
    fn configs_are_summarized_apart_and_failed_runs_are_counted_only() {
        let mut accumulator = SummaryAccumulator::new(1);
        accumulator.push(1, RunStatus::Ok, 100, &[Some(4.0)]);
        accumulator.push(0, RunStatus::Ok, 100, &[Some(1.0)]);
        accumulator.push(0, RunStatus::NonFinite, 100, &[Some(3.0)]);
        accumulator.push(0, RunStatus::Panicked, 40, &[Some(99.0)]);
        accumulator.push(0, RunStatus::Ok, 100, &[None]);
        let rows: Vec<_> = accumulator.rows().collect();
        assert_eq!(rows.len(), 2);
        let first = &rows[0];
        assert_eq!(first.config_id, 0, "rows come in config order");
        assert_eq!((first.runs, first.ok, first.failed), (4, 2, 1));
        assert_eq!(first.ticks.mean, Some(100.0), "the failed run's tick is left out");
        assert_eq!(first.reducers[0].n, 2, "an empty value is left out");
        assert_eq!(first.reducers[0].mean, Some(2.0));
        assert_eq!(rows[1].reducers[0].mean, Some(4.0));
    }
}
