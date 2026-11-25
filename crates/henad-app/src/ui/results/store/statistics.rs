//! Statistics that a results store computes for the views: the Series view's replicate bands, the Response
//! view's lines, the heatmap's cells and the order of the runs table.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::summary::{ReplicateSummary, RunningMoments};

use super::ResultsStore;
use super::axes::ConfigEntry;

/// Spread a series band shows around its centre line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BandKind {
    /// Mean, with one sample standard deviation either side.
    #[default]
    StandardDeviation,
    /// Mean, with the 95% confidence interval of the mean.
    ConfidenceInterval,
    /// Median, from the 10th to the 90th percentile.
    Percentiles,
}

/// Centre line and spread of one stat over the replicates of a config, tick by tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeriesBand {
    pub ticks: Vec<f64>,
    pub center: Vec<f64>,
    pub low: Vec<f64>,
    pub high: Vec<f64>,
    /// Number of runs whose series the band reads.
    pub runs: usize,
}

/// Output plotted against one axis, pooled over every config the pins let through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseQuery {
    pub x_axis: usize,
    /// Index of the reducer column.
    pub output: usize,
    /// Axis whose levels each get their own line.
    pub group_axis: Option<usize>,
    /// Level each axis is held at, `None` for any level. The x and group axes are free whatever their pin.
    pub pins: Vec<Option<usize>>,
}

/// Statistics of an output at one level of the x axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResponsePoint {
    pub x: f64,
    /// Level of the x axis.
    pub level: usize,
    pub summary: ReplicateSummary,
}

/// Points of one line of a response plot.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseLine {
    /// Level of the group axis, `None` without a group axis.
    pub group_level: Option<usize>,
    /// Points in order of x level.
    pub points: Vec<ResponsePoint>,
}

/// Statistic a heatmap cell is coloured by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeatColor {
    #[default]
    Mean,
    StandardDeviation,
    /// Standard deviation over the magnitude of the mean.
    CoefficientOfVariation,
}

/// Output over the levels of two axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatQuery {
    pub x_axis: usize,
    pub y_axis: usize,
    /// Index of the reducer column.
    pub output: usize,
    pub color_by: HeatColor,
    /// Level each axis is held at, `None` for any level. The x and y axes are free whatever their pin.
    pub pins: Vec<Option<usize>>,
}

/// Cells of a heatmap, row by row from the lowest y level.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatGrid {
    /// Number of x levels.
    pub columns: usize,
    /// Number of y levels.
    pub rows: usize,
    /// Value of each cell, not finite for a cell with no value.
    pub values: Vec<f64>,
    /// Number of values each cell pools.
    pub counts: Vec<u64>,
    /// Ids of the configs each cell pools.
    pub configs: Vec<Vec<u64>>,
}

impl HeatGrid {
    /// Returns the index of the cell at x level `column` and y level `row`.
    pub fn cell(&self, column: usize, row: usize) -> usize {
        row * self.columns + column
    }

    /// Returns the lowest and highest finite value, or `None` when no cell holds a finite value.
    pub fn range(&self) -> Option<(f64, f64)> {
        let finite = self.values.iter().copied().filter(|value| value.is_finite());
        finite.fold(None, |range, value| match range {
            None => Some((value, value)),
            Some((low, high)) => Some((f64::min(low, value), f64::max(high, value))),
        })
    }
}

/// Set of runs that the runs table lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunsFilter {
    #[default]
    All,
    /// Runs that ended on a fault or a timeout.
    Failed,
    /// Runs of the selected configs.
    SelectedConfigs,
}

/// Column of the runs table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunsColumn {
    Run,
    Config,
    Replicate,
    Seed,
    Status,
    Ticks,
    /// Wall time spent stepping and sampling.
    Time,
    /// Level of the axis at this index.
    Axis(usize),
    /// Value of the reducer column at this index.
    Output(usize),
}

/// Order of the runs table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunsSort {
    pub column: RunsColumn,
    pub descending: bool,
}

impl Default for RunsSort {
    fn default() -> Self {
        Self {
            column: RunsColumn::Run,
            descending: false,
        }
    }
}

/// Value a run sorts by in one column of the runs table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum SortValue {
    Whole(u64),
    Real(f64),
}

impl SortValue {
    /// Returns the order of `self` and `other`, equal for two values of different kinds.
    pub(super) fn compare(self, other: Self) -> Ordering {
        match (self, other) {
            (Self::Whole(a), Self::Whole(b)) => a.cmp(&b),
            (Self::Real(a), Self::Real(b)) => a.total_cmp(&b),
            (Self::Whole(_), Self::Real(_)) | (Self::Real(_), Self::Whole(_)) => Ordering::Equal,
        }
    }
}

impl ResultsStore {
    /// Returns the lowest and highest finite value of the output column `column`, as in `Infected:max`, over the
    /// runs that did not fail. Returns `None` for a column the runs do not record, or record no value of.
    pub fn output_range(&self, column: &str) -> Option<(f64, f64)> {
        let output = self.reducer_columns.iter().position(|name| name == column)?;
        self.runs
            .iter()
            .filter(|outcome| !outcome.status.is_failure())
            .filter_map(|outcome| outcome.reducers.get(output).copied().flatten())
            .filter(|value| value.is_finite())
            .fold(None, |range, value| match range {
                None => Some((value, value)),
                Some((low, high)) => Some((f64::min(low, value), f64::max(high, value))),
            })
    }

    /// Returns the runs of config `config_id` in order of run id.
    pub fn config_runs(&self, config_id: u64) -> impl Iterator<Item = &RunOutcome> + '_ {
        let positions = self.configs.get(&config_id).map_or(&[][..], |config| &config.runs[..]);
        positions.iter().map(|&position| &self.runs[position])
    }

    /// Returns the id of the first run of config `config_id` that the store holds.
    pub fn first_run_id(&self, config_id: u64) -> Option<u64> {
        self.config_runs(config_id).next().map(|outcome| outcome.run.run_id)
    }

    /// Returns the values of reducer column `output` over the runs of `config` that did not fail, in order of run id.
    pub(super) fn output_values<'a>(
        &'a self,
        config: &'a ConfigEntry,
        output: usize,
    ) -> impl Iterator<Item = f64> + 'a {
        config.runs.iter().filter_map(move |&position| {
            let outcome = &self.runs[position];
            if outcome.status.is_failure() {
                return None;
            }
            outcome.reducers.get(output).copied().flatten()
        })
    }

    /// Returns whether `config` sits at every pinned level of `pins`, the axes in `free` left out.
    pub(super) fn matches_pins(&self, config: &ConfigEntry, pins: &[Option<usize>], free: &[Option<usize>]) -> bool {
        pins.iter().enumerate().all(|(axis, pin)| {
            free.contains(&Some(axis)) || pin.is_none_or(|level| self.entry_level(config, axis) == Some(level))
        })
    }

    /// Returns the band of stat column `stat` over the runs of config `config_id` whose series is held and that did
    /// not fail, or `None` when there is no such run.
    ///
    /// A run that ended early adds nothing to the ticks past its end.
    pub fn band(&self, config_id: u64, stat: usize, kind: BandKind) -> Option<SeriesBand> {
        let config = self.configs.get(&config_id)?;
        let mut samples: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
        let mut runs = 0;
        for &position in &config.runs {
            let outcome = &self.runs[position];
            let Some(series) = self.series.get(outcome.run.run_id) else {
                continue;
            };
            if outcome.status.is_failure() || stat >= series.width() {
                continue;
            }
            runs += 1;
            for (tick, row) in series.rows() {
                if row[stat].is_finite() {
                    samples.entry(tick).or_default().push(row[stat]);
                }
            }
        }
        if runs == 0 {
            return None;
        }
        let mut band = SeriesBand {
            runs,
            ..SeriesBand::default()
        };
        for (tick, mut values) in samples {
            let (center, low, high) = spread(&mut values, kind);
            band.ticks.push(tick as f64);
            band.center.push(center);
            band.low.push(low);
            band.high.push(high);
        }
        Some(band)
    }

    /// Returns the response that `query` requests, one line per level of its group axis.
    ///
    /// A point pools the runs of every config at its x level that the pins let through, failed runs left out. Only a
    /// block that varies the x axis counts. A block that holds it at one level, as each block of a One at a time
    /// design does for every parameter but its own, would pool its variation into that one point.
    pub fn response(&self, query: &ResponseQuery) -> Vec<ResponseLine> {
        let free = [Some(query.x_axis), query.group_axis];
        let mut cells: BTreeMap<(Option<usize>, usize), RunningMoments> = BTreeMap::new();
        for config in self.configs.values() {
            if !self.block_varies(config.block, query.x_axis) || !self.matches_pins(config, &query.pins, &free) {
                continue;
            }
            let Some(x_level) = self.entry_level(config, query.x_axis) else {
                continue;
            };
            let group_level = match query.group_axis {
                None => None,
                Some(axis) => match self.entry_level(config, axis) {
                    Some(level) => Some(level),
                    None => continue,
                },
            };
            let moments = cells.entry((group_level, x_level)).or_default();
            for value in self.output_values(config, query.output) {
                moments.push(value);
            }
        }
        let Some(axis) = self.axes.get(query.x_axis) else {
            return Vec::new();
        };
        let mut lines: Vec<ResponseLine> = Vec::new();
        for ((group_level, level), moments) in cells {
            if moments.count() == 0 {
                continue;
            }
            let point = ResponsePoint {
                x: axis.positions[level],
                level,
                summary: moments.summary(),
            };
            match lines.last_mut() {
                Some(last) if last.group_level == group_level => last.points.push(point),
                _ => lines.push(ResponseLine {
                    group_level,
                    points: vec![point],
                }),
            }
        }
        lines
    }

    /// Returns the heatmap that `query` requests, or `None` when its axes are the same or out of range.
    ///
    /// A cell pools the runs of every config at its levels that the pins let through, failed runs left out. Only a
    /// block that varies the x or the y axis counts, and a config that repeats the values of an earlier config of
    /// the cell counts once. A cell with no value, or too few values for its statistic, holds a value that is not
    /// finite.
    pub fn heat_grid(&self, query: &HeatQuery) -> Option<HeatGrid> {
        if query.x_axis == query.y_axis {
            return None;
        }
        let (columns, rows) = (
            self.axes.get(query.x_axis)?.levels.len(),
            self.axes.get(query.y_axis)?.levels.len(),
        );
        let free = [Some(query.x_axis), Some(query.y_axis)];
        let mut moments = vec![RunningMoments::default(); columns * rows];
        let mut configs = vec![Vec::new(); columns * rows];
        let mut pooled_texts: Vec<BTreeSet<&[String]>> = vec![BTreeSet::new(); columns * rows];
        for (&config_id, config) in &self.configs {
            let varies = self.block_varies(config.block, query.x_axis) || self.block_varies(config.block, query.y_axis);
            if !varies || !self.matches_pins(config, &query.pins, &free) {
                continue;
            }
            let (Some(column), Some(row)) = (
                self.entry_level(config, query.x_axis),
                self.entry_level(config, query.y_axis),
            ) else {
                continue;
            };
            let cell = row * columns + column;
            if !pooled_texts[cell].insert(config.texts.as_slice()) {
                continue;
            }
            configs[cell].push(config_id);
            for value in self.output_values(config, query.output) {
                moments[cell].push(value);
            }
        }
        let values = moments
            .iter()
            .map(|moments| {
                let (mean, deviation) = (moments.mean(), moments.standard_deviation());
                let value = match query.color_by {
                    HeatColor::Mean => mean,
                    HeatColor::StandardDeviation => deviation,
                    HeatColor::CoefficientOfVariation => mean
                        .zip(deviation)
                        .filter(|&(mean, _)| mean != 0.0)
                        .map(|(mean, deviation)| deviation / mean.abs()),
                };
                value.unwrap_or(f64::NAN)
            })
            .collect();
        Some(HeatGrid {
            columns,
            rows,
            values,
            counts: moments.iter().map(RunningMoments::count).collect(),
            configs,
        })
    }

    /// Returns the positions of the runs `filter` lets through, in the order that `sort` requests, ties by run id.
    ///
    /// A missing value sorts last in either direction.
    pub fn row_order(&self, filter: RunsFilter, selected: &BTreeSet<u64>, sort: RunsSort) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.runs.len())
            .filter(|&position| {
                let outcome = &self.runs[position];
                match filter {
                    RunsFilter::All => true,
                    RunsFilter::Failed => outcome.status.is_failure(),
                    RunsFilter::SelectedConfigs => selected.contains(&outcome.run.config_id),
                }
            })
            .collect();
        order.sort_by(|&a, &b| {
            let (first, second) = (&self.runs[a], &self.runs[b]);
            let ordering = match (self.sort_key(first, sort.column), self.sort_key(second, sort.column)) {
                (Some(x), Some(y)) if sort.descending => y.compare(x),
                (Some(x), Some(y)) => x.compare(y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            ordering.then(first.run.run_id.cmp(&second.run.run_id))
        });
        order
    }

    /// Returns the value `outcome` sorts by in `column`, `None` when `outcome` has no value in `column`.
    pub(super) fn sort_key(&self, outcome: &RunOutcome, column: RunsColumn) -> Option<SortValue> {
        let run = &outcome.run;
        match column {
            RunsColumn::Run => Some(SortValue::Whole(run.run_id)),
            RunsColumn::Config => Some(SortValue::Whole(run.config_id)),
            RunsColumn::Replicate => Some(SortValue::Whole(run.rep)),
            RunsColumn::Seed => Some(SortValue::Whole(run.seed)),
            RunsColumn::Status => Some(SortValue::Whole(outcome.status as u64)),
            RunsColumn::Ticks => Some(SortValue::Whole(outcome.ticks)),
            RunsColumn::Time => Some(outcome.wall_ms)
                .filter(|time| time.is_finite())
                .map(SortValue::Real),
            RunsColumn::Axis(axis) => self
                .config_level(run.config_id, axis)
                .map(|level| SortValue::Whole(level as u64)),
            RunsColumn::Output(output) => outcome.reducers.get(output).copied().flatten().map(SortValue::Real),
        }
    }
}

/// Returns the centre and the low and high ends of the spread `kind` over `values`.
///
/// # Panics
///
/// Panics when `values` is empty and `kind` is [`BandKind::Percentiles`].
pub(super) fn spread(values: &mut [f64], kind: BandKind) -> (f64, f64, f64) {
    let mut moments = RunningMoments::default();
    for &value in values.iter() {
        moments.push(value);
    }
    let mean = moments.mean().unwrap_or(f64::NAN);
    match kind {
        BandKind::StandardDeviation => {
            let deviation = moments.standard_deviation().unwrap_or(0.0);
            (mean, mean - deviation, mean + deviation)
        }
        BandKind::ConfidenceInterval => {
            let (low, high) = moments.summary().ci95.unwrap_or((mean, mean));
            (mean, low, high)
        }
        BandKind::Percentiles => {
            values.sort_by(f64::total_cmp);
            (
                percentile(values, 0.5),
                percentile(values, 0.1),
                percentile(values, 0.9),
            )
        }
    }
}

/// Returns the `fraction` percentile of the sorted `values`, interpolated linearly between the two nearest ranks.
pub(super) fn percentile(values: &[f64], fraction: f64) -> f64 {
    let rank = (values.len() - 1) as f64 * fraction;
    let below = rank.floor() as usize;
    let above = (below + 1).min(values.len() - 1);
    values[below] + (rank - below as f64) * (values[above] - values[below])
}
