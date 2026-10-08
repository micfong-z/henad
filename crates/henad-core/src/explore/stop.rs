//! Stop conditions, each ending a run at the first sample where one stat column passes a threshold.
//!
//! A condition reads `<column> <comparator> <threshold>`, as in `Infected <= 0`.

use std::fmt;
use std::str::FromStr;

use crate::explore::reducer::names_a_stat;
use crate::export::StatColumns;
use crate::view::StatDescriptor;

/// Test between a value and a threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparator {
    /// Less than, written `<`.
    Less,
    /// Less than or equal to, written `<=`.
    LessOrEqual,
    /// Greater than, written `>`.
    Greater,
    /// Greater than or equal to, written `>=`.
    GreaterOrEqual,
    /// Equal to, written `==`.
    Equal,
    /// Not equal to, written `!=`.
    NotEqual,
}

impl Comparator {
    /// Every comparator, each two-character symbol before the one-character symbol it starts with.
    const PARSE_ORDER: [Self; 6] = [
        Self::LessOrEqual,
        Self::Less,
        Self::GreaterOrEqual,
        Self::Greater,
        Self::Equal,
        Self::NotEqual,
    ];

    /// Returns the comparator's symbol, as in `<=`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
            Self::Equal => "==",
            Self::NotEqual => "!=",
        }
    }

    /// Returns whether `value` compares to `threshold` as the comparator specifies, by IEEE 754 rules.
    pub fn compare(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Less => value < threshold,
            Self::LessOrEqual => value <= threshold,
            Self::Greater => value > threshold,
            Self::GreaterOrEqual => value >= threshold,
            Self::Equal => value == threshold,
            Self::NotEqual => value != threshold,
        }
    }
}

/// A comparator and its threshold, as in `<=10`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Comparison {
    /// Test between a value and [`Self::threshold`].
    pub comparator: Comparator,
    /// Value a sample is compared with.
    pub threshold: f64,
}

impl Comparison {
    /// Returns whether `value` passes the comparison. A NaN value never passes, under any comparator.
    pub fn holds(self, value: f64) -> bool {
        !value.is_nan() && self.comparator.compare(value, self.threshold)
    }

    /// Checks that the threshold is finite, as a threshold parsed with [`FromStr`] always is.
    ///
    /// # Errors
    ///
    /// Returns [`ComparisonError::BadThreshold`] for a threshold that is infinite or NaN.
    pub fn check(self) -> Result<(), ComparisonError> {
        if self.threshold.is_finite() {
            Ok(())
        } else {
            Err(ComparisonError::BadThreshold {
                raw: self.threshold.to_string(),
            })
        }
    }
}

impl fmt::Display for Comparison {
    /// Writes the comparator and the threshold with no space between, as in `<=10`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.comparator.as_str(), self.threshold)
    }
}

impl FromStr for Comparison {
    type Err = ComparisonError;

    /// Reads a comparator followed by a finite threshold, with optional spaces around the comparator and the threshold.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let text = raw.trim();
        let (comparator, threshold) = Comparator::PARSE_ORDER
            .iter()
            .find_map(|&comparator| {
                text.strip_prefix(comparator.as_str())
                    .map(|rest| (comparator, rest.trim()))
            })
            .ok_or_else(|| ComparisonError::MissingComparator { raw: text.to_owned() })?;
        let threshold = threshold
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .ok_or_else(|| ComparisonError::BadThreshold {
                raw: threshold.to_owned(),
            })?;
        Ok(Self { comparator, threshold })
    }
}

/// Text that cannot be parsed as a [`Comparison`], or a threshold that is not finite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComparisonError {
    /// Text that does not start with a comparator.
    MissingComparator {
        /// Text as written, after trimming.
        raw: String,
    },
    /// A threshold that is not a finite number.
    BadThreshold {
        /// Threshold as written, after trimming, or as `f64`'s `Display` writes it.
        raw: String,
    },
}

impl fmt::Display for ComparisonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingComparator { raw } if raw.is_empty() => {
                write!(f, "missing comparator, expected <, <=, >, >=, == or !=")
            }
            Self::MissingComparator { raw } => write!(f, "'{raw}' does not start with <, <=, >, >=, == or !="),
            Self::BadThreshold { raw } => write!(f, "threshold '{raw}' is not a finite number"),
        }
    }
}

impl std::error::Error for ComparisonError {}

/// A stop condition as written, with its column given as text.
#[derive(Debug, Clone, PartialEq)]
pub struct StopSpec {
    /// Stat column name, or a bare vector or histogram label for its magnitude or total.
    pub column: String,
    /// Test that the column's value has to pass to end the run.
    pub comparison: Comparison,
    /// First tick at which the condition can end a run.
    pub min_tick: u64,
}

impl StopSpec {
    /// Reads `condition`, such as `Infected <= 0`, to be checked from `min_tick` on.
    ///
    /// The comparator is the last run of `<`, `>`, `=` and `!` characters, and the column is the text before it,
    /// trimmed. A column name can then hold spaces and comparator characters, as in `Agents (k=3) <= 0.5`.
    /// A threshold holds no comparator character, so a condition written by [`StopSpec`] is parsed back into the same
    /// spec.
    ///
    /// # Errors
    ///
    /// Returns [`StopError::MissingColumn`] for a condition that starts with its comparator, and
    /// [`StopError::Comparison`] for one with no comparator or a threshold that is not a finite number.
    pub fn parse(condition: &str, min_tick: u64) -> Result<Self, StopError> {
        let is_comparator = |character: char| matches!(character, '<' | '>' | '=' | '!');
        let split = condition.rfind(is_comparator).map_or(condition.len(), |last| {
            condition[..last].trim_end_matches(is_comparator).len()
        });
        let (column, comparison) = condition.split_at(split);
        let column = column.trim();
        if column.is_empty() {
            return Err(StopError::MissingColumn {
                raw: condition.to_owned(),
            });
        }
        let comparison = comparison.parse().map_err(|source| StopError::Comparison {
            raw: condition.to_owned(),
            source,
        })?;
        Ok(Self {
            column: column.to_owned(),
            comparison,
            min_tick,
        })
    }

    /// Checks that the threshold is finite, as [`StopSpec::parse`] does for a condition written as text.
    ///
    /// # Errors
    ///
    /// Returns [`StopError::NonFiniteThreshold`] for a threshold that is infinite or NaN.
    pub fn check_threshold(&self) -> Result<(), StopError> {
        if self.comparison.threshold.is_finite() {
            Ok(())
        } else {
            Err(StopError::NonFiniteThreshold { raw: self.to_string() })
        }
    }

    /// Checks the column against the stat labels a model declares, as [`ReducerSpec::check_label`] does.
    ///
    /// [`ReducerSpec::check_label`]: crate::explore::reducer::ReducerSpec::check_label
    ///
    /// # Errors
    ///
    /// Returns [`StopError::UnknownColumn`] for a column that does not start with a stat label.
    pub fn check_label(&self, stats: &[StatDescriptor]) -> Result<(), StopError> {
        if names_a_stat(&self.column, stats) {
            Ok(())
        } else {
            Err(StopError::UnknownColumn {
                column: self.column.clone(),
                known: stats.iter().map(|stat| stat.label.to_owned()).collect(),
            })
        }
    }
}

impl fmt::Display for StopSpec {
    /// Writes the condition as [`StopSpec::parse`] reads it, as in `Infected <= 0`, without the minimum tick.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Comparison { comparator, threshold } = self.comparison;
        write!(f, "{} {} {threshold}", self.column, comparator.as_str())
    }
}

/// A stop condition that cannot be read or bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopError {
    /// A condition with nothing before its comparator.
    MissingColumn {
        /// Condition as written.
        raw: String,
    },
    /// A condition whose comparison cannot be read.
    Comparison {
        /// Condition as written.
        raw: String,
        /// Reason the comparison cannot be read.
        source: ComparisonError,
    },
    /// A condition built with a threshold that is infinite or NaN.
    NonFiniteThreshold {
        /// Condition as [`StopSpec`]'s `Display` writes it.
        raw: String,
    },
    /// A column that no stat series produces.
    UnknownColumn {
        /// Column as written in the condition.
        column: String,
        /// Stat labels the model declares, or the column names once a build provides them.
        known: Vec<String>,
    },
}

impl fmt::Display for StopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingColumn { raw } => write!(f, "stop condition '{raw}' names no column"),
            Self::Comparison { raw, .. } => {
                write!(
                    f,
                    "invalid stop condition '{raw}', expected COLUMN COMPARATOR THRESHOLD"
                )
            }
            Self::NonFiniteThreshold { raw } => {
                write!(f, "stop condition '{raw}' has a threshold that is not a finite number")
            }
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

impl std::error::Error for StopError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Comparison { source, .. } => Some(source),
            Self::MissingColumn { .. } | Self::NonFiniteThreshold { .. } | Self::UnknownColumn { .. } => None,
        }
    }
}

/// A stop condition bound to one column of a stat layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopCondition {
    column: usize,
    comparison: Comparison,
    min_tick: u64,
}

impl StopCondition {
    /// Binds `spec` to its column of `columns`.
    ///
    /// # Errors
    ///
    /// Returns [`StopError::UnknownColumn`] for a column that [`StatColumns::resolve`] cannot find.
    pub fn bind(spec: &StopSpec, columns: &StatColumns) -> Result<Self, StopError> {
        let column = columns.resolve(&spec.column).ok_or_else(|| StopError::UnknownColumn {
            column: spec.column.clone(),
            known: (0..columns.len())
                .map(|column| columns.name(column).to_owned())
                .collect(),
        })?;
        Ok(Self {
            column,
            comparison: spec.comparison,
            min_tick: spec.min_tick,
        })
    }

    /// Stat column the condition reads.
    pub fn column(&self) -> usize {
        self.column
    }

    /// Returns whether the sample `row`, taken at `tick`, ends the run.
    ///
    /// `row` holds a value per stat column.
    ///
    /// # Panics
    ///
    /// Panics when `row` holds no value at [`Self::column`].
    pub fn holds(&self, tick: u64, row: &[f64]) -> bool {
        tick >= self.min_tick && self.comparison.holds(row[self.column])
    }
}

#[cfg(test)]
mod tests {
    use super::{Comparator, Comparison, ComparisonError, StopCondition, StopError, StopSpec};
    use crate::export::StatColumns;
    use crate::helpers::stat;
    use crate::view::StatDescriptor;

    const COLOR: [u8; 4] = [0, 0, 0, 255];

    fn stop(condition: &str) -> StopSpec {
        StopSpec::parse(condition, 0).expect("a well-formed condition")
    }

    #[test]
    fn a_stop_condition_parses_labels_with_spaces() {
        let spec = stop("Giant Component Share >= 0.5");
        assert_eq!(spec.column, "Giant Component Share");
        assert_eq!(
            spec.comparison,
            Comparison {
                comparator: Comparator::GreaterOrEqual,
                threshold: 0.5
            }
        );
        assert_eq!(spec.to_string(), "Giant Component Share >= 0.5");
        let tight = stop("  Infected<=0 ");
        assert_eq!((tight.column.as_str(), tight.comparison.threshold), ("Infected", 0.0));
        assert_eq!(tight.to_string(), "Infected <= 0");
    }

    /// A condition on a label holding a comparator character splits at the comparator after the label. Otherwise a
    /// spec recorded with a stop condition on that label could not be read back.
    #[test]
    fn a_label_holding_comparator_characters_reads_back() {
        for label in ["Agents (k=3)", "R>1 cells", "a<b", "Not!", "x >= y"] {
            for comparator in Comparator::PARSE_ORDER {
                let spec = StopSpec {
                    column: label.to_owned(),
                    comparison: Comparison {
                        comparator,
                        threshold: -0.5,
                    },
                    min_tick: 3,
                };
                assert_eq!(StopSpec::parse(&spec.to_string(), 3), Ok(spec.clone()), "{spec}");
            }
        }
        assert_eq!(stop("R>1 cells<=0.5").column, "R>1 cells");
    }

    #[test]
    fn every_comparator_parses() {
        for comparator in Comparator::PARSE_ORDER {
            let spec = stop(&format!("Infected {} -3.5", comparator.as_str()));
            assert_eq!(spec.comparison.comparator, comparator);
            assert_eq!(spec.comparison.threshold, -3.5);
            let comparison = Comparison {
                comparator,
                threshold: 10.0,
            };
            assert_eq!(comparison.to_string().parse(), Ok(comparison), "{comparison}");
        }
        let holds = |comparator, value| {
            Comparison {
                comparator,
                threshold: 1.0,
            }
            .holds(value)
        };
        assert!(holds(Comparator::Less, 0.5) && !holds(Comparator::Less, 1.0));
        assert!(holds(Comparator::LessOrEqual, 1.0) && !holds(Comparator::LessOrEqual, 1.5));
        assert!(holds(Comparator::Greater, 1.5) && !holds(Comparator::Greater, 1.0));
        assert!(holds(Comparator::GreaterOrEqual, 1.0) && !holds(Comparator::GreaterOrEqual, 0.5));
        assert!(holds(Comparator::Equal, 1.0) && !holds(Comparator::Equal, 0.5));
        assert!(holds(Comparator::NotEqual, 0.5) && !holds(Comparator::NotEqual, 1.0));
    }

    #[test]
    fn a_malformed_condition_is_refused() {
        assert_eq!(
            StopSpec::parse("<= 0", 0),
            Err(StopError::MissingColumn { raw: "<= 0".to_owned() })
        );
        let comparison_error = |condition: &str| match StopSpec::parse(condition, 0) {
            Err(StopError::Comparison { source, .. }) => source,
            other => panic!("{condition} gave {other:?}"),
        };
        assert!(matches!(
            comparison_error("Infected 0"),
            ComparisonError::MissingComparator { .. }
        ));
        assert!(matches!(
            comparison_error("Infected => 0"),
            ComparisonError::MissingComparator { .. }
        ));
        assert!(matches!(
            comparison_error("Infected = 0"),
            ComparisonError::MissingComparator { .. }
        ));
        assert_eq!(
            comparison_error("Infected <= many"),
            ComparisonError::BadThreshold { raw: "many".to_owned() }
        );
        assert!(matches!(
            comparison_error("Infected < NaN"),
            ComparisonError::BadThreshold { .. }
        ));
        assert_eq!(
            StopSpec::parse("Infected <= x", 0).map_err(|error| error.to_string()),
            Err("invalid stop condition 'Infected <= x', expected COLUMN COMPARATOR THRESHOLD".to_owned())
        );
    }

    /// The fragment an error quotes is trimmed, and a missing comparator is reported as missing.
    #[test]
    fn a_missing_comparator_quotes_the_trimmed_text() {
        assert_eq!(
            " 0 ".parse::<Comparison>(),
            Err(ComparisonError::MissingComparator { raw: "0".to_owned() })
        );
        let error = "".parse::<Comparison>().expect_err("no comparator");
        assert_eq!(error.to_string(), "missing comparator, expected <, <=, >, >=, == or !=");
    }

    /// A comparison built outside [`std::str::FromStr`] can hold an infinite threshold. Unchecked, it would hold at
    /// the first sample, and its spec written back as text could not be parsed.
    #[test]
    fn a_threshold_that_is_not_finite_is_refused() {
        for threshold in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let spec = StopSpec {
                column: "Infected".to_owned(),
                comparison: Comparison {
                    comparator: Comparator::LessOrEqual,
                    threshold,
                },
                min_tick: 0,
            };
            assert_eq!(
                spec.check_threshold(),
                Err(StopError::NonFiniteThreshold { raw: spec.to_string() })
            );
            assert!(StopSpec::parse(&spec.to_string(), 0).is_err(), "{spec}");
        }
        let infinite = StopSpec {
            comparison: Comparison {
                comparator: Comparator::LessOrEqual,
                threshold: f64::INFINITY,
            },
            ..stop("Infected <= 0")
        };
        assert_eq!(
            infinite.check_threshold().map_err(|error| error.to_string()),
            Err("stop condition 'Infected <= inf' has a threshold that is not a finite number".to_owned())
        );
        assert_eq!(stop("Infected <= 0").check_threshold(), Ok(()));
    }

    #[test]
    fn nan_never_satisfies_a_stop_condition() {
        for comparator in Comparator::PARSE_ORDER {
            let comparison = Comparison {
                comparator,
                threshold: 0.0,
            };
            assert!(!comparison.holds(f64::NAN), "{comparison}");
        }
        let columns = StatColumns::plan(&[stat("Infected", 0.0, COLOR)]);
        let condition = StopCondition::bind(&stop("Infected != 5"), &columns).expect("the column exists");
        assert!(!condition.holds(10, &[f64::NAN]));
        assert!(condition.holds(10, &[4.0]));
    }

    #[test]
    fn a_stop_is_not_checked_before_its_min_tick() {
        let columns = StatColumns::plan(&[stat("Recovered", 0.0, COLOR), stat("Infected", 0.0, COLOR)]);
        let spec = StopSpec::parse("Infected <= 0", 20).expect("a well-formed condition");
        let condition = StopCondition::bind(&spec, &columns).expect("the column exists");
        assert_eq!(condition.column(), 1);
        assert!(!condition.holds(0, &[5.0, 0.0]));
        assert!(!condition.holds(19, &[5.0, 0.0]));
        assert!(condition.holds(20, &[5.0, 0.0]));
        assert!(!condition.holds(25, &[5.0, 1.0]));
    }

    #[test]
    fn a_stop_over_an_unknown_column_is_refused() {
        let stats = [StatDescriptor::new("Infected", COLOR)];
        assert_eq!(stop("Infected <= 0").check_label(&stats), Ok(()));
        assert!(stop("Recovered <= 0").check_label(&stats).is_err());
        let columns = StatColumns::plan(&[stat("Infected", 0.0, COLOR)]);
        let error = StopCondition::bind(&stop("Infected.x <= 0"), &columns).expect_err("no such column");
        assert_eq!(
            error.to_string(),
            "unknown stat column 'Infected.x', expected one of Infected"
        );
    }
}
