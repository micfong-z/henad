//! Design tables, the comma-separated form of a table design.
//!
//! The header names one parameter id or `action.<name>` per column, and each row after it is one config. A config
//! keeps the fixed value of every parameter and action the table leaves out.

use std::fmt;

use crate::explore::factor::{Factor, FactorDomain, FactorLevel, FactorSlot};
use crate::explore::spec::{ACTION_COLUMN_PREFIX, ActionSpec};
use crate::explore::value::{ValueError, parse_value};
use crate::export::csv::{CsvError, parse_records};
use crate::params::ParamDescriptor;

/// Byte order mark some spreadsheets write before the header.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// Reads the design table `text` into one factor per column, whose levels are the column's values in row order.
///
/// A zip over the factors gives one config per row. Spaces around a field are ignored, and so are blank lines.
///
/// # Errors
///
/// Returns [`DesignTableError`] for text that is not CSV or holds no row after its header, a column that names no
/// parameter or action or names one twice, a row of another width than the header, or a value its column refuses.
pub fn read_table(
    text: &str,
    params: &[ParamDescriptor],
    actions: &[ActionSpec],
) -> Result<Vec<Factor>, DesignTableError> {
    let records = parse_records(text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(text)).map_err(DesignTableError::Csv)?;
    let mut records = (1_usize..)
        .zip(records)
        .filter(|(_, record)| !matches!(record.as_slice(), [field] if field.trim().is_empty()));
    let Some((_, header)) = records.next() else {
        return Err(DesignTableError::NoHeader);
    };

    let mut factors: Vec<Factor> = Vec::with_capacity(header.len());
    for column in &header {
        let column = column.trim();
        let slot = column_slot(column, params, actions)?;
        if factors.iter().any(|factor| factor.slot == slot) {
            return Err(DesignTableError::DuplicateColumn {
                column: column.to_owned(),
            });
        }
        factors.push(Factor {
            slot,
            domain: FactorDomain::Levels(Vec::new()),
        });
    }

    let mut rows = 0;
    for (row, record) in records {
        if record.len() != header.len() {
            return Err(DesignTableError::Width {
                row,
                expected: header.len(),
                found: record.len(),
            });
        }
        for ((factor, column), field) in factors.iter_mut().zip(&header).zip(&record) {
            let level = table_level(factor.slot, field.trim(), params).map_err(|source| DesignTableError::Value {
                row,
                column: column.trim().to_owned(),
                source,
            })?;
            if let FactorDomain::Levels(levels) = &mut factor.domain {
                levels.push(level);
            }
        }
        rows += 1;
    }
    if rows == 0 {
        return Err(DesignTableError::NoRows);
    }
    Ok(factors)
}

/// Returns the slot the column named `column` writes.
fn column_slot(
    column: &str,
    params: &[ParamDescriptor],
    actions: &[ActionSpec],
) -> Result<FactorSlot, DesignTableError> {
    let slot = match column.strip_prefix(ACTION_COLUMN_PREFIX) {
        Some(name) => actions
            .iter()
            .position(|action| action.name == name)
            .map(FactorSlot::Action),
        None => params
            .iter()
            .position(|descriptor| descriptor.id == column)
            .map(FactorSlot::Param),
    };
    slot.ok_or_else(|| DesignTableError::UnknownColumn {
        column: column.to_owned(),
        known: params
            .iter()
            .map(|descriptor| descriptor.id.to_owned())
            .chain(actions.iter().map(ActionSpec::column_name))
            .collect(),
    })
}

/// Returns the level `field` gives the factor writing `slot`.
fn table_level(
    slot: FactorSlot,
    field: &str,
    params: &[ParamDescriptor],
) -> Result<FactorLevel, DesignTableValueError> {
    match slot {
        FactorSlot::Param(index) => parse_value(&params[index].kind, field)
            .map(FactorLevel::Param)
            .map_err(DesignTableValueError::Param),
        FactorSlot::Action(_) => field
            .parse()
            .ok()
            .map(FactorLevel::Tick)
            .ok_or_else(|| DesignTableValueError::Tick { raw: field.to_owned() }),
    }
}

/// A design table that gives no configs. Rows count from 1 at the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignTableError {
    /// Text that is not CSV, for the reason inside.
    Csv(CsvError),
    /// Text with no header.
    NoHeader,
    /// A header with no row after it.
    NoRows,
    /// A column that names no parameter id and no `action.<name>`. `known` lists the columns there can be.
    UnknownColumn { column: String, known: Vec<String> },
    /// A column named twice.
    DuplicateColumn { column: String },
    /// Row `row`, with `found` fields where the header has `expected`.
    Width { row: usize, expected: usize, found: usize },
    /// A value in row `row` that column `column` refuses, for the reason in `source`.
    Value {
        row: usize,
        column: String,
        source: DesignTableValueError,
    },
}

impl fmt::Display for DesignTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Csv(_) => f.write_str("design table is not valid CSV"),
            Self::NoHeader => f.write_str("design table is empty"),
            Self::NoRows => f.write_str("design table has a header and no rows"),
            Self::UnknownColumn { column, known } => {
                write!(
                    f,
                    "unknown design table column '{column}', expected one of {}",
                    known.join(", ")
                )
            }
            Self::DuplicateColumn { column } => write!(f, "design table has column '{column}' twice"),
            Self::Width { row, expected, found } => {
                write!(
                    f,
                    "row {row} of the design table has {found} fields, expected {expected}"
                )
            }
            Self::Value { row, column, .. } => write!(f, "row {row} of the design table, column '{column}'"),
        }
    }
}

impl std::error::Error for DesignTableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Csv(error) => Some(error),
            Self::Value { source, .. } => Some(source),
            Self::NoHeader
            | Self::NoRows
            | Self::UnknownColumn { .. }
            | Self::DuplicateColumn { .. }
            | Self::Width { .. } => None,
        }
    }
}

/// A value of a design table that its column refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignTableValueError {
    /// A value the parameter's descriptor refuses, for the reason inside.
    Param(ValueError),
    /// An action tick that is not a whole number from 0.
    Tick { raw: String },
}

impl fmt::Display for DesignTableValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Param(error) => error.fmt(f),
            Self::Tick { raw } => write!(f, "tick '{raw}' is not a non-negative integer"),
        }
    }
}

impl std::error::Error for DesignTableValueError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Param(error) => error.source(),
            Self::Tick { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DesignTableError, DesignTableValueError, read_table};
    use crate::explore::design::{Block, DesignKind, generate};
    use crate::explore::factor::{FactorLevel, FactorSlot};
    use crate::explore::plan::Config;
    use crate::explore::spec::ActionSpec;
    use crate::explore::value::format_value;
    use crate::export::csv::escape_field;
    use crate::helpers::{bool_param, choice_param, f32_param, u32_param};
    use crate::params::{ParamDescriptor, ParamValue};

    const NEIGHBORHOODS: &[&str] = &["moore", "von neumann, wide"];

    fn params() -> Vec<ParamDescriptor> {
        vec![
            f32_param("rate", "Rate", 0.5, 0.0, 1.0, Some(0.01)),
            u32_param("size", "Size", 10, 1, 100),
            bool_param("wrap", "Wrap", true),
            choice_param("neighborhood", "Neighborhood", NEIGHBORHOODS, 0),
        ]
    }

    fn actions() -> Vec<ActionSpec> {
        vec![ActionSpec::new("seed_outbreak", 40)]
    }

    fn base() -> Config {
        Config {
            block: 0,
            params: params().iter().map(|param| param.kind.default_value()).collect(),
            action_ticks: vec![40],
        }
    }

    /// Returns the configs of the table `text`.
    fn configs(text: &str) -> Result<Vec<Config>, DesignTableError> {
        let factors = read_table(text, &params(), &actions())?;
        let block = Block {
            design: DesignKind::Table { text: text.to_owned() },
            factors,
            design_seed: 0,
        };
        Ok(generate(&block, &base()).expect("a table zips"))
    }

    /// Returns `configs` as a design table over every parameter and action.
    fn table(configs: &[Config]) -> String {
        let params = params();
        let mut header: Vec<String> = params.iter().map(|param| param.id.to_owned()).collect();
        header.extend(actions().iter().map(ActionSpec::column_name));
        let mut lines = vec![header.join(",")];
        for config in configs {
            let mut fields: Vec<String> = params
                .iter()
                .zip(&config.params)
                .map(|(param, value)| escape_field(&format_value(&param.kind, value)))
                .collect();
            fields.extend(config.action_ticks.iter().map(ToString::to_string));
            lines.push(fields.join(","));
        }
        lines.join("\n")
    }

    #[test]
    fn each_row_is_one_config() {
        let text = "\u{feff}size, action.seed_outbreak\r\n4,10\r\n\r\n 8 , 20\r\n";
        let configs = configs(text).expect("a valid table");
        assert_eq!(configs.len(), 2, "the blank line is skipped");
        assert_eq!(configs[1].params[1], ParamValue::U32(8));
        assert_eq!(configs[1].action_ticks, [20]);
        assert_eq!(
            configs[0].params[0],
            ParamValue::F32(0.5),
            "a column left out keeps its value"
        );
    }

    #[test]
    fn a_design_table_round_trips() {
        let text = "rate,neighborhood,action.seed_outbreak,wrap\n\
                    0.1,moore,10,true\n\
                    0.30000001,\"von neumann, wide\",0,false\n\
                    1,1,18446744073709551615,true\n";
        let first = configs(text).expect("a valid table");
        assert_eq!(first[1].params[0], ParamValue::F32(0.3));
        assert_eq!(
            first[1].params[3],
            ParamValue::Choice(1),
            "a quoted option keeps its comma"
        );
        assert_eq!(first[2].params[3], ParamValue::Choice(1), "an option by index");
        assert_eq!(first[2].action_ticks, [u64::MAX]);
        let written = table(&first);
        assert_eq!(configs(&written), Ok(first), "{written}");
    }

    #[test]
    fn a_design_table_with_an_unknown_column_is_refused() {
        let error = configs("rate,speed\n0.1,2\n").expect_err("no parameter 'speed'");
        assert_eq!(
            error.to_string(),
            "unknown design table column 'speed', expected one of rate, size, wrap, neighborhood, action.seed_outbreak"
        );
        assert!(matches!(
            configs("action.second_wave\n4\n"),
            Err(DesignTableError::UnknownColumn { .. })
        ));
        assert!(
            matches!(
                configs("seed_outbreak\n4\n"),
                Err(DesignTableError::UnknownColumn { .. })
            ),
            "an action column carries its prefix"
        );
    }

    #[test]
    fn a_malformed_design_table_is_refused() {
        assert_eq!(configs(""), Err(DesignTableError::NoHeader));
        assert_eq!(configs("rate,size\n\n"), Err(DesignTableError::NoRows));
        assert_eq!(
            configs("rate,rate\n0.1,0.2\n"),
            Err(DesignTableError::DuplicateColumn {
                column: "rate".to_owned()
            })
        );
        assert_eq!(
            configs("rate,size\n0.1,2\n0.2\n"),
            Err(DesignTableError::Width {
                row: 3,
                expected: 2,
                found: 1
            })
        );
        assert!(matches!(configs("rate\n\"0.1\n"), Err(DesignTableError::Csv(_))));
        let refused = |text: &str| match configs(text) {
            Err(DesignTableError::Value { row, column, source }) => (row, column, source),
            other => panic!("{text} gave {other:?}"),
        };
        let (row, column, source) = refused("size,rate\n4,0.1\n5,1.5\n");
        assert_eq!((row, column.as_str()), (3, "rate"));
        assert!(matches!(source, DesignTableValueError::Param(_)), "{source:?}");
        let (_, column, source) = refused("action.seed_outbreak\n-4\n");
        assert_eq!(column, "action.seed_outbreak");
        assert_eq!(source, DesignTableValueError::Tick { raw: "-4".to_owned() });
    }

    #[test]
    fn table_columns_name_their_slots() {
        let factors = read_table("wrap,action.seed_outbreak\ntrue,3\n", &params(), &actions()).expect("a valid table");
        let slots: Vec<FactorSlot> = factors.iter().map(|factor| factor.slot).collect();
        assert_eq!(slots, [FactorSlot::Param(2), FactorSlot::Action(0)]);
        assert_eq!(factors[1].levels(), Some(&[FactorLevel::Tick(3)][..]));
    }
}
