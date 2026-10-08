//! Sampling of one run: the ticks it samples, and the reducer values and series it keeps from them.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::explore::reducer::{ReducerError, ReducerKind, ReducerPlan, ReducerState};
use crate::explore::spec::{MeasureSettings, RunSettings};
use crate::explore::stop::{StopCondition, StopError};
use crate::export::{StatColumns, StatsWriteError};
use crate::view::StatEntry;

/// Sampled ticks of a run, and the columns, reducers and stop condition each sample feeds.
///
/// A run samples tick `warmup + j * stats_every` for every `j` that does not pass the total, and the total itself.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurePlan {
    warmup: u64,
    total: u64,
    stats_every: u64,
    series_every: u64,
    columns: StatColumns,
    reducers: ReducerPlan,
    stop: Option<StopCondition>,
}

impl MeasurePlan {
    /// Checks the length and timeout of `run`, the cadence of `measure`, the threshold of every comparison and every
    /// window, before any column is known.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when the total tick count overflows, the timeout does not read back from the seconds
    /// a spec file records, `stats_every` is 0, `series_every` is not a multiple of `stats_every`, the stop condition
    /// or a `first` reducer compares against a threshold that is not finite, or a `mean@` reducer's window ends
    /// before it starts.
    pub fn check(run: &RunSettings, measure: &MeasureSettings) -> Result<(), MeasureError> {
        if run.warmup.checked_add(run.steps).is_none() {
            return Err(MeasureError::TooManyTicks {
                warmup: run.warmup,
                steps: run.steps,
            });
        }
        if let Some(timeout) = run.timeout
            && Duration::try_from_secs_f64(timeout.as_secs_f64()).is_err()
        {
            return Err(MeasureError::TimeoutTooLong { timeout });
        }
        if measure.stats_every == 0 {
            return Err(MeasureError::ZeroStatsEvery);
        }
        if !measure.series_every.is_multiple_of(measure.stats_every) {
            return Err(MeasureError::SeriesOffCadence {
                series_every: measure.series_every,
                stats_every: measure.stats_every,
            });
        }
        if let Some(stop) = &run.stop {
            stop.check_threshold().map_err(MeasureError::Stop)?;
        }
        for reducer in &measure.reducers {
            match reducer.kind {
                ReducerKind::FirstCrossing(comparison) => comparison.check().map_err(|source| {
                    MeasureError::Reducer(ReducerError::Comparison {
                        raw: reducer.kind.to_string(),
                        source,
                    })
                })?,
                ReducerKind::WindowMean { start, end } if start > end => {
                    return Err(MeasureError::Reducer(ReducerError::BadWindow {
                        raw: reducer.kind.to_string(),
                    }));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Returns the plan for runs of `run` sampled as `measure` asks, over the stat layout `columns`.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when [`Self::check`] refuses the settings, or a reducer or the stop condition does
    /// not bind to `columns`.
    pub fn new(run: &RunSettings, measure: &MeasureSettings, columns: StatColumns) -> Result<Self, MeasureError> {
        Self::check(run, measure)?;
        let reducers =
            ReducerPlan::bind(&columns, &measure.reducers, measure.default_reducers).map_err(MeasureError::Reducer)?;
        let stop = run
            .stop
            .as_ref()
            .map(|stop| StopCondition::bind(stop, &columns))
            .transpose()
            .map_err(MeasureError::Stop)?;
        Ok(Self {
            warmup: run.warmup,
            total: run.warmup + run.steps,
            stats_every: measure.stats_every,
            series_every: measure.series_every,
            columns,
            reducers,
            stop,
        })
    }

    pub fn warmup(&self) -> u64 {
        self.warmup
    }

    /// Tick every run ends on, the warm-up included.
    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn stats_every(&self) -> u64 {
        self.stats_every
    }

    /// Ticks between two rows of the series, or 0 when no series is kept.
    pub fn series_every(&self) -> u64 {
        self.series_every
    }

    pub fn columns(&self) -> &StatColumns {
        &self.columns
    }

    pub fn reducers(&self) -> &ReducerPlan {
        &self.reducers
    }

    pub fn stop(&self) -> Option<&StopCondition> {
        self.stop.as_ref()
    }

    /// First sampled tick.
    pub fn first_sample(&self) -> u64 {
        self.warmup
    }

    /// Returns the first sampled tick after `tick`, or `None` from the total on.
    pub fn next_sample(&self, tick: u64) -> Option<u64> {
        if tick >= self.total {
            return None;
        }
        if tick < self.warmup {
            return Some(self.warmup);
        }
        let intervals = (tick - self.warmup) / self.stats_every + 1;
        let next = self.warmup.saturating_add(intervals.saturating_mul(self.stats_every));
        Some(next.min(self.total))
    }

    /// Returns whether a run samples `tick`.
    pub fn is_sample(&self, tick: u64) -> bool {
        tick == self.total
            || (tick >= self.warmup && tick < self.total && (tick - self.warmup).is_multiple_of(self.stats_every))
    }

    /// Returns every sampled tick in order.
    pub fn sample_ticks(&self) -> impl Iterator<Item = u64> + '_ {
        std::iter::successors(Some(self.first_sample()), |&tick| self.next_sample(tick))
    }

    /// Number of samples a run that reaches the total takes.
    pub fn sample_count(&self) -> u64 {
        count_on_cadence(self.total - self.warmup, self.stats_every)
    }

    /// Rows of the series a run that reaches the total keeps.
    pub fn series_row_count(&self) -> u64 {
        if self.series_every == 0 {
            0
        } else {
            count_on_cadence(self.total - self.warmup, self.series_every)
        }
    }

    /// Returns whether the series keeps the sample at `tick` on its cadence alone.
    fn keeps_in_series(&self, tick: u64) -> bool {
        self.series_every != 0 && tick >= self.warmup && (tick - self.warmup).is_multiple_of(self.series_every)
    }
}

/// Returns the number of ticks in `0..=span` that are multiples of `every`, counting `span` itself once.
///
/// The count saturates at `u64::MAX`.
fn count_on_cadence(span: u64, every: u64) -> u64 {
    (span / every).saturating_add(1 + u64::from(!span.is_multiple_of(every)))
}

/// Settings that cannot measure a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasureError {
    /// A warm-up and step count whose sum overflows a tick.
    TooManyTicks { warmup: u64, steps: u64 },
    /// A timeout too long to read back from the seconds a spec file records, as [`Duration::MAX`] is.
    TimeoutTooLong { timeout: Duration },
    /// A `stats_every` of 0.
    ZeroStatsEvery,
    /// A `series_every` that is not a multiple of `stats_every`.
    SeriesOffCadence { series_every: u64, stats_every: u64 },
    /// A reducer that is refused or does not bind, for the reason inside.
    Reducer(ReducerError),
    /// A stop condition that is refused or does not bind, for the reason inside.
    Stop(StopError),
}

impl fmt::Display for MeasureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyTicks { warmup, steps } => {
                write!(
                    f,
                    "a warm-up of {warmup} ticks plus {steps} steps exceeds the largest tick"
                )
            }
            Self::TimeoutTooLong { timeout } => {
                write!(
                    f,
                    "a timeout of {} seconds is too long to record",
                    timeout.as_secs_f64()
                )
            }
            Self::ZeroStatsEvery => write!(f, "stats_every must be at least 1"),
            Self::SeriesOffCadence {
                series_every,
                stats_every,
            } => write!(
                f,
                "series_every {series_every} is not a multiple of stats_every {stats_every}"
            ),
            Self::Reducer(_) => write!(f, "reducer"),
            Self::Stop(_) => write!(f, "stop condition"),
        }
    }
}

impl std::error::Error for MeasureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Reducer(error) => Some(error),
            Self::Stop(error) => Some(error),
            _ => None,
        }
    }
}

/// Stat rows of one run, each a tick and a value per column, stored row after row in one buffer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeriesBuffer {
    width: usize,
    ticks: Vec<u64>,
    values: Vec<f64>,
}

impl SeriesBuffer {
    /// Returns an empty buffer for rows of `width` values.
    pub fn new(width: usize) -> Self {
        Self {
            width,
            ticks: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Values in each row.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.ticks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    /// Appends the row `values` sampled at `tick`.
    ///
    /// # Panics
    ///
    /// Panics when `values` is not [`Self::width`] long.
    pub fn push(&mut self, tick: u64, values: &[f64]) {
        assert_eq!(values.len(), self.width, "a series row holds one value per column");
        self.ticks.push(tick);
        self.values.extend_from_slice(values);
    }

    /// Frees the buffer's spare capacity, leaving room for its rows alone.
    pub fn shrink_to_fit(&mut self) {
        self.ticks.shrink_to_fit();
        self.values.shrink_to_fit();
    }

    pub fn ticks(&self) -> &[u64] {
        &self.ticks
    }

    /// Returns the values of row `i`.
    ///
    /// # Panics
    ///
    /// Panics when `i` is not below [`Self::len`].
    pub fn row(&self, i: usize) -> &[f64] {
        &self.values[i * self.width..(i + 1) * self.width]
    }

    /// Returns every row as its tick and its values.
    pub fn rows(&self) -> impl Iterator<Item = (u64, &[f64])> + '_ {
        self.ticks.iter().enumerate().map(|(i, &tick)| (tick, self.row(i)))
    }
}

/// First stat value of a run that was not finite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonFiniteSample {
    /// Stat column name, before CSV escaping.
    pub column: String,
    pub tick: u64,
}

impl fmt::Display for NonFiniteSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not finite at tick {}", self.column, self.tick)
    }
}

/// Values a run kept from its samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    /// One value per reducer, `None` for a reducer that saw no finite value, a `first` reducer whose comparison never
    /// held, or a window mean with no sample in its window.
    pub reducers: Vec<Option<f64>>,
    pub series: SeriesBuffer,
    /// First value that was not finite, when any sample had one.
    pub non_finite: Option<NonFiniteSample>,
}

/// Folds the samples of one run into reducer values and a series, and checks the stop condition at each.
///
/// The series keeps a sample on its `series_every` cadence, and [`Sampler::finish`] adds the last sample when the
/// cadence missed it. A run that stops therefore ends its series on the sample where the condition held.
#[derive(Debug, Clone)]
pub struct Sampler {
    plan: Arc<MeasurePlan>,
    reducers: ReducerState,
    series: SeriesBuffer,
    /// Values of the latest sample, one per column.
    row: Vec<f64>,
    /// Values of the sample being read, swapped into `row` once they fit the layout.
    incoming: Vec<f64>,
    /// Tick of the latest sample when the series has not kept it.
    unkept_tick: Option<u64>,
    non_finite: Option<NonFiniteSample>,
}

impl Sampler {
    pub fn new(plan: Arc<MeasurePlan>) -> Self {
        Self {
            reducers: ReducerState::new(&plan.reducers),
            series: SeriesBuffer::new(plan.columns.len()),
            row: Vec::with_capacity(plan.columns.len()),
            incoming: Vec::with_capacity(plan.columns.len()),
            unkept_tick: None,
            non_finite: None,
            plan,
        }
    }

    pub fn plan(&self) -> &MeasurePlan {
        &self.plan
    }

    /// Takes the sample `stats`, read at `tick`, and returns whether the stop condition holds there.
    ///
    /// `tick` is one of the plan's sampled ticks, taken in increasing order. A run whose stop condition holds ends
    /// on `tick`, and takes no later sample.
    ///
    /// # Errors
    ///
    /// Returns [`StatsWriteError::Shape`] when `stats` no longer fits the plan's column layout. The samples taken
    /// before it are kept.
    pub fn push(&mut self, tick: u64, stats: &[StatEntry]) -> Result<bool, StatsWriteError> {
        debug_assert!(self.plan.is_sample(tick), "tick {tick} is not a sampled tick");
        self.plan.columns.extract(tick, stats, &mut self.incoming)?;
        std::mem::swap(&mut self.row, &mut self.incoming);
        self.unkept_tick = None;
        if self.non_finite.is_none()
            && let Some(column) = self.row.iter().position(|value| !value.is_finite())
        {
            self.non_finite = Some(NonFiniteSample {
                column: self.plan.columns.name(column).to_owned(),
                tick,
            });
        }
        self.reducers.push(&self.plan.reducers, tick, &self.row);
        if self.plan.keeps_in_series(tick) {
            self.series.push(tick, &self.row);
        } else if self.plan.series_every != 0 {
            self.unkept_tick = Some(tick);
        }
        Ok(self.plan.stop.is_some_and(|stop| stop.holds(tick, &self.row)))
    }

    /// Returns the reducer values and the series, with the last sample added to the series if its cadence missed it.
    pub fn finish(mut self) -> Measured {
        if let Some(tick) = self.unkept_tick {
            self.series.push(tick, &self.row);
        }
        Measured {
            reducers: self.reducers.finish(&self.plan.reducers),
            series: self.series,
            non_finite: self.non_finite,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{MeasureError, MeasurePlan, NonFiniteSample, Sampler};
    use crate::explore::reducer::{ReducerError, ReducerKind, ReducerSpec};
    use crate::explore::spec::{MeasureSettings, RunSettings};
    use crate::explore::stop::{Comparator, Comparison, ComparisonError, StopError, StopSpec};
    use crate::export::StatColumns;
    use crate::helpers::stat;
    use crate::view::StatEntry;

    const COLOR: [u8; 4] = [0, 0, 0, 255];

    fn sample(infected: f64, recovered: f64) -> Vec<StatEntry> {
        vec![stat("Infected", infected, COLOR), stat("Recovered", recovered, COLOR)]
    }

    fn plan(warmup: u64, steps: u64, stats_every: u64, series_every: u64) -> MeasurePlan {
        let run = RunSettings {
            steps,
            warmup,
            ..RunSettings::default()
        };
        let measure = MeasureSettings {
            stats_every,
            series_every,
            ..MeasureSettings::default()
        };
        MeasurePlan::new(&run, &measure, StatColumns::plan(&sample(0.0, 0.0))).expect("a valid cadence")
    }

    #[test]
    fn sampled_ticks_include_the_final_tick() {
        let off_cadence = plan(3, 10, 4, 4);
        assert_eq!(off_cadence.sample_ticks().collect::<Vec<_>>(), [3, 7, 11, 13]);
        assert_eq!(off_cadence.sample_count(), 4);
        assert!(off_cadence.is_sample(13) && off_cadence.is_sample(7));
        assert!(!off_cadence.is_sample(2) && !off_cadence.is_sample(12) && !off_cadence.is_sample(15));
        assert_eq!(off_cadence.next_sample(0), Some(3));
        assert_eq!(off_cadence.next_sample(12), Some(13));
        assert_eq!(off_cadence.next_sample(13), None);

        let on_cadence = plan(0, 8, 4, 4);
        assert_eq!(on_cadence.sample_ticks().collect::<Vec<_>>(), [0, 4, 8]);
        assert_eq!(on_cadence.sample_count(), 3);

        let no_steps = plan(5, 0, 4, 4);
        assert_eq!(no_steps.sample_ticks().collect::<Vec<_>>(), [5]);
        assert_eq!(no_steps.sample_count(), 1);
    }

    #[test]
    fn the_series_is_thinned_and_ends_on_the_final_sample() {
        let thinned = Arc::new(plan(0, 10, 2, 4));
        let mut sampler = Sampler::new(Arc::clone(&thinned));
        for tick in thinned.sample_ticks() {
            sampler.push(tick, &sample(tick as f64, 0.0)).expect("the layout holds");
        }
        let measured = sampler.finish();
        assert_eq!(measured.series.ticks(), [0, 4, 8, 10]);
        assert_eq!(thinned.series_row_count(), 4);
        assert_eq!(measured.series.row(3), [10.0, 0.0]);
        let rows: Vec<(u64, Vec<f64>)> = measured.series.rows().map(|(tick, row)| (tick, row.to_vec())).collect();
        assert_eq!(rows[1], (4, vec![4.0, 0.0]));

        let silent = Arc::new(plan(0, 10, 2, 0));
        let mut sampler = Sampler::new(Arc::clone(&silent));
        for tick in silent.sample_ticks() {
            sampler.push(tick, &sample(1.0, 0.0)).expect("the layout holds");
        }
        let measured = sampler.finish();
        assert!(measured.series.is_empty(), "a series_every of 0 keeps no rows");
        assert_eq!(silent.series_row_count(), 0);
        assert_eq!(measured.reducers.len(), 8, "reducers still run");
    }

    #[test]
    fn non_finite_samples_are_skipped_and_flagged() {
        let plan = Arc::new(plan(0, 4, 2, 2));
        let mut sampler = Sampler::new(plan);
        sampler.push(0, &sample(3.0, 1.0)).expect("the layout holds");
        sampler.push(2, &sample(f64::NAN, 1.0)).expect("the layout holds");
        sampler.push(4, &sample(4.0, f64::INFINITY)).expect("the layout holds");
        let measured = sampler.finish();
        assert_eq!(
            measured.non_finite,
            Some(NonFiniteSample {
                column: "Infected".to_owned(),
                tick: 2
            }),
            "the first one is reported"
        );
        assert_eq!(
            measured.reducers[..4],
            [Some(4.0), Some(3.0), Some(4.0), Some(3.5)],
            "Infected skips its NaN"
        );
        assert_eq!(
            measured.reducers[4..],
            [Some(1.0), Some(1.0), Some(1.0), Some(1.0)],
            "Recovered skips its infinity"
        );
        assert!(
            measured.series.row(1)[0].is_nan(),
            "the series keeps what the model gave"
        );
        assert_eq!(
            measured.non_finite.map(|sample| sample.to_string()).as_deref(),
            Some("Infected is not finite at tick 2")
        );
    }

    #[test]
    fn a_sample_of_another_shape_is_refused() {
        let mut sampler = Sampler::new(Arc::new(plan(0, 4, 2, 2)));
        sampler.push(0, &sample(1.0, 1.0)).expect("the layout holds");
        assert!(sampler.push(2, &[stat("Infected", 1.0, COLOR)]).is_err());
        let measured = sampler.finish();
        assert_eq!(measured.series.ticks(), [0], "the refused sample adds no row");

        let mut sampler = Sampler::new(Arc::new(plan(0, 8, 2, 4)));
        sampler.push(0, &sample(1.0, 1.0)).expect("the layout holds");
        sampler.push(2, &sample(2.0, 1.0)).expect("the layout holds");
        assert!(sampler.push(4, &[stat("Infected", 1.0, COLOR)]).is_err());
        let measured = sampler.finish();
        assert_eq!(
            measured.series.ticks(),
            [0, 2],
            "the sample the series had not kept yet outlives the refused one"
        );
        assert_eq!(measured.series.row(1), [2.0, 1.0]);
    }

    #[test]
    fn a_cadence_that_cannot_sample_is_refused() {
        let run = RunSettings::default();
        let measure = |stats_every, series_every| MeasureSettings {
            stats_every,
            series_every,
            ..MeasureSettings::default()
        };
        assert_eq!(
            MeasurePlan::check(&run, &measure(0, 0)),
            Err(MeasureError::ZeroStatsEvery)
        );
        assert_eq!(
            MeasurePlan::check(&run, &measure(4, 6)),
            Err(MeasureError::SeriesOffCadence {
                series_every: 6,
                stats_every: 4
            })
        );
        assert_eq!(MeasurePlan::check(&run, &measure(4, 0)), Ok(()));
        assert_eq!(MeasurePlan::check(&run, &measure(4, 8)), Ok(()));
        let endless = RunSettings {
            steps: u64::MAX,
            warmup: 1,
            ..RunSettings::default()
        };
        assert!(MeasurePlan::check(&endless, &measure(1, 1)).is_err());
    }

    /// The regression. A reversed window and a timeout past what seconds in an `f64` read back as both planned, and
    /// the spec a manifest recorded for them did not read back.
    #[test]
    fn settings_a_spec_file_cannot_record_are_refused() {
        let reversed = MeasureSettings {
            reducers: vec![ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::WindowMean { start: 600, end: 200 },
            }],
            ..MeasureSettings::default()
        };
        assert_eq!(
            MeasurePlan::check(&RunSettings::default(), &reversed),
            Err(MeasureError::Reducer(ReducerError::BadWindow {
                raw: "mean@600..200".to_owned()
            }))
        );
        let single_tick = MeasureSettings {
            reducers: vec![ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::WindowMean { start: 200, end: 200 },
            }],
            ..MeasureSettings::default()
        };
        assert_eq!(MeasurePlan::check(&RunSettings::default(), &single_tick), Ok(()));

        let endless = RunSettings {
            timeout: Some(Duration::MAX),
            ..RunSettings::default()
        };
        assert_eq!(
            MeasurePlan::check(&endless, &MeasureSettings::default()),
            Err(MeasureError::TimeoutTooLong { timeout: Duration::MAX })
        );
        let long = RunSettings {
            timeout: Some(Duration::from_secs(u64::MAX / 2)),
            ..RunSettings::default()
        };
        assert_eq!(MeasurePlan::check(&long, &MeasureSettings::default()), Ok(()));
    }

    /// The regression. The longest run a check accepts, sampled every tick, overflowed the count of its samples.
    #[test]
    fn the_longest_accepted_run_counts_its_samples_without_overflow() {
        let longest = plan(0, u64::MAX, 1, 1);
        assert_eq!(longest.sample_count(), u64::MAX, "the count saturates");
        assert_eq!(longest.series_row_count(), u64::MAX);
        assert_eq!(plan(0, u64::MAX, 2, 2).series_row_count(), u64::MAX / 2 + 2);
    }

    /// The regression. A threshold built outside the parser could be infinite, and every run stopped at its first
    /// sample.
    #[test]
    fn a_comparison_against_a_threshold_that_is_not_finite_is_refused() {
        let infinite = Comparison {
            comparator: Comparator::LessOrEqual,
            threshold: f64::INFINITY,
        };
        let run = RunSettings {
            stop: Some(StopSpec {
                column: "Infected".to_owned(),
                comparison: infinite,
                min_tick: 0,
            }),
            ..RunSettings::default()
        };
        assert_eq!(
            MeasurePlan::check(&run, &MeasureSettings::default()),
            Err(MeasureError::Stop(StopError::NonFiniteThreshold {
                raw: "Infected <= inf".to_owned()
            }))
        );

        let measure = MeasureSettings {
            reducers: vec![ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::FirstCrossing(Comparison {
                    threshold: f64::NAN,
                    ..infinite
                }),
            }],
            ..MeasureSettings::default()
        };
        assert_eq!(
            MeasurePlan::check(&RunSettings::default(), &measure),
            Err(MeasureError::Reducer(ReducerError::Comparison {
                raw: "first<=NaN".to_owned(),
                source: ComparisonError::BadThreshold { raw: "NaN".to_owned() },
            }))
        );
        assert!(
            MeasurePlan::new(&run, &MeasureSettings::default(), StatColumns::plan(&sample(0.0, 0.0))).is_err(),
            "a plan built without a check refuses it too"
        );
    }

    #[test]
    fn the_sampler_reports_the_first_sample_where_the_stop_holds() {
        let run = RunSettings {
            steps: 20,
            stop: Some(StopSpec::parse("Infected <= 0", 6).expect("a well-formed condition")),
            ..RunSettings::default()
        };
        let measure = MeasureSettings {
            stats_every: 2,
            series_every: 4,
            ..MeasureSettings::default()
        };
        let plan = MeasurePlan::new(&run, &measure, StatColumns::plan(&sample(0.0, 0.0))).expect("the column exists");
        let mut sampler = Sampler::new(Arc::new(plan));
        let mut stops = Vec::new();
        for (tick, infected) in [(0, 0.0), (2, 3.0), (4, 1.0), (6, f64::NAN), (8, 2.0), (10, 0.0)] {
            stops.push(sampler.push(tick, &sample(infected, 0.0)).expect("the layout holds"));
        }
        assert_eq!(
            stops,
            [false, false, false, false, false, true],
            "tick 0 is before the minimum tick, and NaN never stops"
        );
        let measured = sampler.finish();
        assert_eq!(
            measured.series.ticks(),
            [0, 4, 8, 10],
            "the series ends on the stopping sample"
        );
        assert_eq!(measured.reducers[0], Some(0.0), "Infected:final is the stopping value");

        let missing = RunSettings {
            stop: Some(StopSpec::parse("Susceptible <= 0", 0).expect("a well-formed condition")),
            ..RunSettings::default()
        };
        assert!(matches!(
            MeasurePlan::new(&missing, &measure, StatColumns::plan(&sample(0.0, 0.0))),
            Err(MeasureError::Stop(StopError::UnknownColumn { .. }))
        ));
    }
}
