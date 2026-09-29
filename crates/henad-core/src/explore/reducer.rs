//! Reducers that fold a run's samples of one stat column into one value.
//!
//! A reducer's output column is named `<stat column>:<kind>`, as in `Infected:max`, `Infected:first<=10` or
//! `Recovered:mean@200..600`.

use std::fmt;
use std::str::FromStr;

use crate::explore::stop::{Comparison, ComparisonError};
use crate::export::StatColumns;
use crate::view::StatDescriptor;

/// Fold a reducer applies to its column's samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReducerKind {
    /// Last finite sampled value.
    Final,
    Min,
    Max,
    Mean,
    /// First sampled tick of the greatest value.
    ArgMax,
    /// First sampled tick of the least value.
    ArgMin,
    /// First sampled tick whose value passes the comparison, empty when none does.
    FirstCrossing(Comparison),
    /// Mean of the samples from tick `start` to tick `end` inclusive.
    WindowMean {
        start: u64,
        end: u64,
    },
}

impl ReducerKind {
    /// Kinds every column other than a histogram bucket gets by default.
    pub const DEFAULTS: [Self; 4] = [Self::Final, Self::Min, Self::Max, Self::Mean];
}

impl fmt::Display for ReducerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Final => f.write_str("final"),
            Self::Min => f.write_str("min"),
            Self::Max => f.write_str("max"),
            Self::Mean => f.write_str("mean"),
            Self::ArgMax => f.write_str("argmax"),
            Self::ArgMin => f.write_str("argmin"),
            Self::FirstCrossing(comparison) => write!(f, "first{comparison}"),
            Self::WindowMean { start, end } => write!(f, "mean@{start}..{end}"),
        }
    }
}

impl FromStr for ReducerKind {
    type Err = ReducerError;

    /// Reads a kind as [`ReducerKind`]'s `Display` writes it.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "final" => return Ok(Self::Final),
            "min" => return Ok(Self::Min),
            "max" => return Ok(Self::Max),
            "mean" => return Ok(Self::Mean),
            "argmax" => return Ok(Self::ArgMax),
            "argmin" => return Ok(Self::ArgMin),
            _ => {}
        }
        if let Some(comparison) = raw.strip_prefix("first") {
            return comparison
                .parse()
                .map(Self::FirstCrossing)
                .map_err(|source| ReducerError::Comparison {
                    raw: raw.to_owned(),
                    source,
                });
        }
        if let Some(window) = raw.strip_prefix("mean@") {
            let tick = |text: &str| text.parse::<u64>().ok();
            return window
                .split_once("..")
                .and_then(|(start, end)| Some((tick(start)?, tick(end)?)))
                .filter(|(start, end)| start <= end)
                .map(|(start, end)| Self::WindowMean { start, end })
                .ok_or_else(|| ReducerError::BadWindow { raw: raw.to_owned() });
        }
        Err(ReducerError::UnknownKind { raw: raw.to_owned() })
    }
}

/// Returns whether `column` names a series of `stats`, as a label alone or followed by `.` and a component.
pub(crate) fn names_a_stat(column: &str, stats: &[StatDescriptor]) -> bool {
    stats.iter().any(|stat| {
        column
            .strip_prefix(stat.label)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
}

/// A reducer as written, naming its column by text.
#[derive(Debug, Clone, PartialEq)]
pub struct ReducerSpec {
    /// Stat column name, or a bare vector or histogram label for its magnitude or total.
    pub column: String,
    pub kind: ReducerKind,
}

impl ReducerSpec {
    /// Checks the column against the stat labels a model declares.
    ///
    /// A column passes when it is a label, or a label followed by `.` and a component. [`ReducerPlan::bind`] checks
    /// the column again once a build has given the full column list.
    ///
    /// # Errors
    ///
    /// Returns [`ReducerError::UnknownColumn`] for a column no label starts.
    pub fn check_label(&self, stats: &[StatDescriptor]) -> Result<(), ReducerError> {
        if names_a_stat(&self.column, stats) {
            Ok(())
        } else {
            Err(ReducerError::UnknownColumn {
                column: self.column.clone(),
                known: stats.iter().map(|stat| stat.label.to_owned()).collect(),
            })
        }
    }
}

impl FromStr for ReducerSpec {
    type Err = ReducerError;

    /// Reads `COLUMN:KIND`, split at the last `:`.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (column, kind) = raw
            .rsplit_once(':')
            .ok_or_else(|| ReducerError::MissingKind { raw: raw.to_owned() })?;
        Ok(Self {
            column: column.to_owned(),
            kind: kind.parse()?,
        })
    }
}

/// A reducer that cannot be read or bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReducerError {
    /// A reducer with no `:` between the column and the kind.
    MissingKind { raw: String },
    /// A kind no reducer has.
    UnknownKind { raw: String },
    /// A `first` kind whose comparison cannot be read, for the reason in `source`.
    Comparison { raw: String, source: ComparisonError },
    /// A `mean@` kind whose window is not `START..END`, with `START` at most `END`.
    BadWindow { raw: String },
    /// A column no stat series gives. `known` lists the columns or labels there are.
    UnknownColumn { column: String, known: Vec<String> },
}

impl fmt::Display for ReducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKind { raw } => write!(f, "invalid reducer '{raw}', expected COLUMN:KIND"),
            Self::UnknownKind { raw } => write!(
                f,
                "unknown reducer kind '{raw}', expected one of final, min, max, mean, argmax, argmin, first<=VALUE, \
                 mean@START..END"
            ),
            Self::Comparison { raw, .. } => write!(
                f,
                "invalid reducer kind '{raw}', expected 'first' followed by <, <=, >, >=, == or != and a number"
            ),
            Self::BadWindow { raw } => write!(f, "invalid reducer kind '{raw}', expected mean@START..END"),
            Self::UnknownColumn { column, known } => {
                write!(
                    f,
                    "unknown stat column '{column}', expected one of {}",
                    known.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ReducerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Comparison { source, .. } => Some(source),
            Self::MissingKind { .. }
            | Self::UnknownKind { .. }
            | Self::BadWindow { .. }
            | Self::UnknownColumn { .. } => None,
        }
    }
}

/// Reducers bound to the columns of one stat layout, in output order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReducerPlan {
    columns: Vec<usize>,
    kinds: Vec<ReducerKind>,
    names: Vec<String>,
}

impl ReducerPlan {
    /// Binds the default reducers when `defaults` is set, then each of `specs`, to columns of `columns`.
    ///
    /// The defaults come column by column. A reducer already bound, by default or by an earlier spec, is not bound
    /// again.
    ///
    /// # Errors
    ///
    /// Returns [`ReducerError::UnknownColumn`] for a spec whose column [`StatColumns::resolve`] cannot find.
    pub fn bind(columns: &StatColumns, specs: &[ReducerSpec], defaults: bool) -> Result<Self, ReducerError> {
        let mut plan = Self::default();
        if defaults {
            for column in (0..columns.len()).filter(|&column| !columns.is_bucket(column)) {
                for kind in ReducerKind::DEFAULTS {
                    plan.add(columns, column, kind);
                }
            }
        }
        for spec in specs {
            let column = columns
                .resolve(&spec.column)
                .ok_or_else(|| ReducerError::UnknownColumn {
                    column: spec.column.clone(),
                    known: (0..columns.len())
                        .map(|column| columns.name(column).to_owned())
                        .collect(),
                })?;
            plan.add(columns, column, spec.kind);
        }
        Ok(plan)
    }

    fn add(&mut self, columns: &StatColumns, column: usize, kind: ReducerKind) {
        let bound = self
            .columns
            .iter()
            .zip(&self.kinds)
            .any(|(&bound_column, &bound_kind)| bound_column == column && bound_kind == kind);
        if !bound {
            self.columns.push(column);
            self.kinds.push(kind);
            self.names.push(format!("{}:{kind}", columns.name(column)));
        }
    }

    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// Output column names, before CSV escaping.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Stat column reducer `i` reads.
    pub fn column(&self, i: usize) -> usize {
        self.columns[i]
    }

    pub fn kind(&self, i: usize) -> ReducerKind {
        self.kinds[i]
    }
}

/// Running values of every reducer in a [`ReducerPlan`], over one run.
#[derive(Debug, Clone, PartialEq)]
pub struct ReducerState {
    /// Last, least or greatest value so far, the sum for a mean, or the tick of a first crossing.
    values: Vec<f64>,
    /// Tick of the value kept by a [`ReducerKind::ArgMax`] or [`ReducerKind::ArgMin`] reducer.
    ticks: Vec<u64>,
    /// Finite values folded in so far, the finite values inside the window for a window mean, or 1 once a first
    /// crossing is found.
    counts: Vec<u64>,
}

impl ReducerState {
    pub fn new(plan: &ReducerPlan) -> Self {
        Self {
            values: vec![0.0; plan.len()],
            ticks: vec![0; plan.len()],
            counts: vec![0; plan.len()],
        }
    }

    /// Folds in the sample taken at `tick`, where `row` holds a value per stat column. A value that is not finite
    /// is skipped.
    pub fn push(&mut self, plan: &ReducerPlan, tick: u64, row: &[f64]) {
        for (i, ((value, kept_tick), count)) in self
            .values
            .iter_mut()
            .zip(&mut self.ticks)
            .zip(&mut self.counts)
            .enumerate()
        {
            let sample = row[plan.columns[i]];
            if !sample.is_finite() {
                continue;
            }
            let replaces = match plan.kinds[i] {
                ReducerKind::Final => true,
                ReducerKind::Min | ReducerKind::ArgMin => *count == 0 || sample < *value,
                ReducerKind::Max | ReducerKind::ArgMax => *count == 0 || sample > *value,
                ReducerKind::Mean => {
                    *value += sample;
                    false
                }
                ReducerKind::FirstCrossing(comparison) => {
                    if *count == 0 && comparison.holds(sample) {
                        *value = tick as f64;
                        *count = 1;
                    }
                    continue;
                }
                ReducerKind::WindowMean { start, end } => {
                    if tick < start || tick > end {
                        continue;
                    }
                    *value += sample;
                    false
                }
            };
            if replaces {
                *value = sample;
                *kept_tick = tick;
            }
            *count += 1;
        }
    }

    /// Returns each reducer's value, or `None` for a reducer that saw no finite value, or no crossing.
    pub fn finish(&self, plan: &ReducerPlan) -> Vec<Option<f64>> {
        self.values
            .iter()
            .zip(&self.ticks)
            .zip(&self.counts)
            .zip(&plan.kinds)
            .map(|(((&value, &tick), &count), kind)| match (count, kind) {
                (0, _) => None,
                (_, ReducerKind::Mean | ReducerKind::WindowMean { .. }) => Some(value / count as f64),
                (_, ReducerKind::ArgMax | ReducerKind::ArgMin) => Some(tick as f64),
                _ => Some(value),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{ReducerError, ReducerKind, ReducerPlan, ReducerSpec, ReducerState};
    use crate::export::StatColumns;
    use crate::helpers::{stat, stat_histogram, stat_vec2};
    use crate::view::StatDescriptor;

    const COLOR: [u8; 4] = [0, 0, 0, 255];

    fn columns() -> StatColumns {
        StatColumns::plan(&[
            stat("Infected", 0.0, COLOR),
            stat_vec2("Velocity", 0.0, 0.0, COLOR),
            stat_histogram("Speed", vec![0.0, 1.0, 2.0], vec![0, 0], COLOR),
        ])
    }

    fn spec(raw: &str) -> ReducerSpec {
        raw.parse().expect("a well-formed reducer")
    }

    #[test]
    fn the_core_reducers_match_a_hand_computed_series() {
        let columns = StatColumns::plan(&[stat("A", 0.0, COLOR)]);
        let plan = ReducerPlan::bind(&columns, &[], true).expect("defaults always bind");
        assert_eq!(plan.names(), ["A:final", "A:min", "A:max", "A:mean"]);
        let mut state = ReducerState::new(&plan);
        for (tick, value) in (0..).zip([3.0, 1.0, 4.0, 1.0, 5.0]) {
            state.push(&plan, tick, &[value]);
        }
        assert_eq!(state.finish(&plan), [Some(5.0), Some(1.0), Some(5.0), Some(2.8)]);
    }

    #[test]
    fn a_reducer_that_sees_no_finite_value_is_empty() {
        let columns = StatColumns::plan(&[stat("A", 0.0, COLOR)]);
        let plan = ReducerPlan::bind(&columns, &[], true).expect("defaults always bind");
        let mut state = ReducerState::new(&plan);
        assert_eq!(state.finish(&plan), [None; 4], "no samples");
        state.push(&plan, 0, &[f64::NAN]);
        state.push(&plan, 1, &[f64::INFINITY]);
        assert_eq!(state.finish(&plan), [None; 4], "no finite samples");
    }

    /// Returns the values of the reducers `kinds` over column `A`, fed `(tick, value)` samples.
    fn reduce(kinds: &[&str], samples: &[(u64, f64)]) -> Vec<Option<f64>> {
        let columns = StatColumns::plan(&[stat("A", 0.0, COLOR)]);
        let specs: Vec<ReducerSpec> = kinds.iter().map(|kind| spec(&format!("A:{kind}"))).collect();
        let plan = ReducerPlan::bind(&columns, &specs, false).expect("column A exists");
        let mut state = ReducerState::new(&plan);
        for &(tick, value) in samples {
            state.push(&plan, tick, &[value]);
        }
        state.finish(&plan)
    }

    #[test]
    fn argmax_takes_the_first_tick_of_the_maximum() {
        let samples = [(0, 2.0), (5, 7.0), (10, 1.0), (15, 7.0), (20, 1.0), (25, f64::NAN)];
        assert_eq!(reduce(&["argmax", "argmin"], &samples), [Some(5.0), Some(10.0)]);
        assert_eq!(reduce(&["argmax"], &[]), [None]);
        assert_eq!(
            reduce(&["argmax"], &[(0, f64::NAN), (5, f64::INFINITY)]),
            [None],
            "no finite sample"
        );
    }

    #[test]
    fn first_crossing_reports_the_first_sampled_tick() {
        let samples = [(0, 30.0), (5, 12.0), (10, 9.0), (15, 20.0), (20, 4.0)];
        assert_eq!(
            reduce(&["first<=10", "first>12", "first==12", "first!=30"], &samples),
            [Some(10.0), Some(0.0), Some(5.0), Some(5.0)]
        );
        assert_eq!(
            reduce(&["first<10"], &[(0, f64::NAN), (5, 3.0)]),
            [Some(5.0)],
            "a NaN sample never crosses"
        );
    }

    #[test]
    fn a_crossing_that_never_happens_is_empty() {
        assert_eq!(reduce(&["first<0"], &[(0, 1.0), (5, 0.0), (10, 2.0)]), [None]);
        assert_eq!(reduce(&["first<0"], &[]), [None]);
    }

    #[test]
    fn a_window_mean_covers_only_its_ticks() {
        let samples = [(0, 100.0), (5, 1.0), (10, 2.0), (15, 3.0), (20, 100.0)];
        assert_eq!(
            reduce(&["mean@5..15", "mean@10..10", "mean@21..40", "mean"], &samples),
            [Some(2.0), Some(2.0), None, Some(41.2)]
        );
        assert_eq!(
            reduce(&["mean@0..10"], &[(0, 4.0), (5, f64::NAN), (10, 2.0)]),
            [Some(3.0)],
            "a value that is not finite is skipped"
        );
    }

    #[test]
    fn every_kind_reads_back_from_its_name() {
        for name in [
            "final",
            "min",
            "max",
            "mean",
            "argmax",
            "argmin",
            "first<=10",
            "first>0.5",
            "first==-2",
            "first!=0",
            "mean@200..600",
            "mean@0..0",
        ] {
            let kind: ReducerKind = name.parse().expect("a known kind");
            assert_eq!(kind.to_string(), name);
        }
        assert_eq!(
            spec("Infected:first <= 10").kind.to_string(),
            "first<=10",
            "spaces around the comparator are dropped"
        );
        for bad in [
            "first",
            "first<=x",
            "first=10",
            "mean@",
            "mean@600..200",
            "mean@1..",
            "mean@a..b",
        ] {
            let error = bad.parse::<ReducerKind>().expect_err("refused");
            assert!(
                matches!(error, ReducerError::Comparison { .. } | ReducerError::BadWindow { .. }),
                "{bad} gave {error:?}"
            );
        }
    }

    #[test]
    fn defaults_skip_buckets_and_specs_add_to_them() {
        let columns = columns();
        let plan = ReducerPlan::bind(&columns, &[spec("Infected:max"), spec("Velocity:mean")], true)
            .expect("both columns exist");
        let names: Vec<&str> = plan.names().iter().map(String::as_str).collect();
        assert_eq!(
            names.len(),
            5 * 4,
            "Infected, the three vector parts and the histogram total"
        );
        assert!(
            !names.iter().any(|name| name.starts_with("Speed.[")),
            "no bucket gets a default"
        );
        assert!(names.contains(&"Speed.total:mean"));
        assert_eq!(
            names.iter().filter(|&&name| name == "Infected:max").count(),
            1,
            "a spec repeating a default binds once"
        );

        let plan = ReducerPlan::bind(&columns, &[spec("Velocity:max"), spec("Speed.[0, 1):min")], false)
            .expect("both columns exist");
        assert_eq!(plan.names(), ["Velocity.magnitude:max", "Speed.[0, 1):min"]);
        assert_eq!(plan.kind(1), ReducerKind::Min);
    }

    #[test]
    fn a_reducer_is_read_as_column_and_kind() {
        assert_eq!(
            spec("Giant Component: Share:mean"),
            ReducerSpec {
                column: "Giant Component: Share".to_owned(),
                kind: ReducerKind::Mean,
            }
        );
        assert_eq!(
            "Infected".parse::<ReducerSpec>(),
            Err(ReducerError::MissingKind {
                raw: "Infected".to_owned()
            })
        );
        assert_eq!(
            "Infected:median".parse::<ReducerSpec>(),
            Err(ReducerError::UnknownKind {
                raw: "median".to_owned()
            })
        );
        for kind in ReducerKind::DEFAULTS {
            assert_eq!(kind.to_string().parse(), Ok(kind));
        }
    }

    #[test]
    fn an_unknown_column_is_refused() {
        let error = ReducerPlan::bind(&columns(), &[spec("Recovered:max")], false).expect_err("no such column");
        assert!(
            error
                .to_string()
                .starts_with("unknown stat column 'Recovered', expected one of Infected, "),
            "{error}"
        );

        let stats = [
            StatDescriptor::new("Infected", COLOR),
            StatDescriptor::new("Velocity", COLOR),
        ];
        assert_eq!(spec("Infected:max").check_label(&stats), Ok(()));
        assert_eq!(spec("Velocity.x:max").check_label(&stats), Ok(()));
        assert!(spec("Infectedness:max").check_label(&stats).is_err());
        assert!(spec("Recovered:max").check_label(&stats).is_err());
    }
}
