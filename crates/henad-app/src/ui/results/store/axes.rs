//! Axes and configs of a results store: the value columns that take more than one value across the configs, and
//! the level of each config on them.

use std::collections::{BTreeMap, BTreeSet};

use henad_core::explore::factor::FactorTarget;
use henad_core::explore::plan::{Config, Plan};
use henad_core::explore::spec::ACTION_COLUMN_PREFIX;
use henad_core::explore::value::format_value;
use henad_core::metadata::TimeUnit;
use henad_core::params::ParamDescriptor;
use henad_explore::search_run::SearchUpdate;

use crate::ui::time_axis::RunTime;

use super::ResultsStore;

/// Level id of a config on an axis whose column the config lacks.
pub(super) const NO_LEVEL_ID: usize = usize::MAX;

/// A value column that takes more than one value across the configs.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultsAxis {
    /// Position of the axis among the value columns.
    pub column: usize,
    /// Name of the axis as the views show it.
    pub label: String,
    /// Values of the axis as `runs.csv` writes them. Numbers ascend, and other values keep the order of the first
    /// config that takes them.
    pub levels: Vec<String>,
    /// Place of each level on a plot axis, its value on a numeric axis and its index otherwise.
    pub positions: Vec<f64>,
    /// Whether every level is a number.
    pub numeric: bool,
    /// Id of each level. A level keeps its id when levels are added below it.
    pub(super) level_ids: Vec<usize>,
    /// Level of each id, the inverse of `level_ids`.
    pub(super) id_levels: Vec<usize>,
}

impl ResultsAxis {
    /// Returns the axis of value column `column`, labelled `label`, with `levels` at `positions`.
    pub(super) fn new(column: usize, label: String, levels: Vec<String>, positions: Vec<f64>, numeric: bool) -> Self {
        let level_ids: Vec<usize> = (0..levels.len()).collect();
        Self {
            column,
            label,
            levels,
            positions,
            numeric,
            id_levels: level_ids.clone(),
            level_ids,
        }
    }

    /// Returns the level whose id is `level_id`, or `None` for [`NO_LEVEL_ID`].
    pub(super) fn level_of_id(&self, level_id: usize) -> Option<usize> {
        self.id_levels.get(level_id).copied()
    }

    /// Returns the id of the level whose value is `text`, or [`NO_LEVEL_ID`] when the axis lacks it.
    pub(super) fn level_id(&self, text: &str) -> usize {
        self.level(text).map_or(NO_LEVEL_ID, |level| self.level_ids[level])
    }

    /// Returns the level whose value is `text`, or `None` when the axis lacks it.
    pub(super) fn level(&self, text: &str) -> Option<usize> {
        if !self.numeric {
            return self.levels.iter().position(|level| level == text);
        }
        let value = finite_number(text)?;
        let start = self
            .positions
            .partition_point(|position| position.total_cmp(&value).is_lt());
        (start..self.levels.len())
            .take_while(|&level| self.positions[level].total_cmp(&value).is_eq())
            .find(|&level| self.levels[level] == text)
    }

    /// Adds each value of `texts` that is missing from the axis as a level with its own id.
    ///
    /// A numeric axis places a new level among the numbers, after any old level of the same value, and leaves out a
    /// text that is not a finite number. Any other axis adds new levels at the end. Note that every level above the
    /// lowest new level moves up, in time that grows with the number of levels it moves. A search over an `f32` range
    /// draws a value below the highest level on nearly every batch.
    pub(super) fn extend<'a>(&mut self, texts: impl Iterator<Item = &'a str>) {
        let mut seen = BTreeSet::new();
        let new_texts = texts.filter(|&text| self.level(text).is_none() && seen.insert(text));
        let mut new_levels: Vec<(f64, &str)> = if self.numeric {
            let mut numbers: Vec<(f64, &str)> = new_texts
                .filter_map(|text| Some((finite_number(text)?, text)))
                .collect();
            numbers.sort_by(|a, b| a.0.total_cmp(&b.0));
            numbers
        } else {
            let positions = (self.levels.len()..).map(|level| level as f64);
            positions.zip(new_texts).collect()
        };
        let old_count = self.levels.len();
        let count = old_count + new_levels.len();
        self.levels.resize_with(count, String::new);
        self.positions.resize(count, 0.0);
        self.level_ids.resize(count, NO_LEVEL_ID);
        // Places the new levels from the highest down. Each old level above the one being placed moves up to make
        // room, and the levels below the lowest new one stay where they are.
        let (mut unmoved, mut slot) = (old_count, count);
        while let Some((position, text)) = new_levels.pop() {
            while unmoved > 0 && self.positions[unmoved - 1].total_cmp(&position).is_gt() {
                unmoved -= 1;
                slot -= 1;
                self.levels.swap(unmoved, slot);
                self.positions[slot] = self.positions[unmoved];
                let level_id = self.level_ids[unmoved];
                self.level_ids[slot] = level_id;
                self.id_levels[level_id] = slot;
            }
            slot -= 1;
            self.levels[slot] = text.to_owned();
            self.positions[slot] = position;
            self.level_ids[slot] = self.id_levels.len();
            self.id_levels.push(slot);
        }
    }
}

/// Block and values of one config.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ConfigTexts {
    /// Index of the block the config comes from.
    pub(super) block: usize,
    /// Value of each value column, as `runs.csv` writes it.
    pub(super) texts: Vec<String>,
}

/// One config of the results.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ConfigEntry {
    /// Index of the block the config comes from.
    pub(super) block: usize,
    /// Value of each value column, as `runs.csv` writes it.
    pub(super) texts: Vec<String>,
    /// Id of the config's level on each axis, [`NO_LEVEL_ID`] where the config lacks the axis's column.
    pub(super) level_ids: Vec<usize>,
    /// Positions in the store's runs of the config's runs, in order of run id.
    pub(super) runs: Vec<usize>,
}

/// Levels the configs of one block take on each axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BlockAxes {
    /// Level id on each axis from the first config of the block with a level on that axis, [`NO_LEVEL_ID`] while
    /// no config of the block has a level on it.
    pub(super) first_level_ids: Vec<usize>,
    /// Whether each axis takes more than one level within the block.
    pub(super) varies: Vec<bool>,
}

impl BlockAxes {
    pub(super) fn new(axis_count: usize) -> Self {
        Self {
            first_level_ids: vec![NO_LEVEL_ID; axis_count],
            varies: vec![false; axis_count],
        }
    }

    /// Adds `level_ids`, the level ids of one more config of the block, one per axis.
    pub(super) fn add(&mut self, level_ids: &[usize]) {
        let axes = self.first_level_ids.iter_mut().zip(&mut self.varies);
        for ((first, varies), &level_id) in axes.zip(level_ids) {
            if level_id == NO_LEVEL_ID {
                continue;
            }
            if *first == NO_LEVEL_ID {
                *first = level_id;
            } else if *first != level_id {
                *varies = true;
            }
        }
    }
}

impl ResultsStore {
    /// Replaces the configs with `texts`, the block and values of each config by id, and finds the axes they vary.
    pub(super) fn set_configs(&mut self, texts: BTreeMap<u64, ConfigTexts>) {
        let axes: Vec<ResultsAxis> = (0..self.value_columns.len())
            .filter_map(|column| self.axis(column, &texts))
            .collect();
        let lookups: Vec<BTreeMap<&str, usize>> = axes
            .iter()
            .map(|axis| {
                let levels = axis.levels.iter().enumerate();
                levels.map(|(level, text)| (text.as_str(), level)).collect()
            })
            .collect();
        let configs: BTreeMap<u64, ConfigEntry> = texts
            .into_iter()
            .map(|(config_id, ConfigTexts { block, texts })| {
                // A new axis gives each level the id of its place.
                let level_ids = axes
                    .iter()
                    .zip(&lookups)
                    .map(|(axis, lookup)| {
                        let text = texts.get(axis.column).map(String::as_str);
                        text.and_then(|text| lookup.get(text)).copied().unwrap_or(NO_LEVEL_ID)
                    })
                    .collect();
                let entry = ConfigEntry {
                    block,
                    texts,
                    level_ids,
                    runs: Vec::new(),
                };
                (config_id, entry)
            })
            .collect();
        self.block_axes = block_axes(&configs, axes.len());
        self.configs = configs;
        self.axes = axes;
        self.revision += 1;
    }

    /// Returns whether axis `axis` takes more than one level within block `block`.
    pub(super) fn block_varies(&self, block: usize, axis: usize) -> bool {
        self.block_axes
            .get(&block)
            .and_then(|block_axes| block_axes.varies.get(axis))
            .copied()
            .unwrap_or(false)
    }

    /// Returns the axis of value column `column` across the configs `texts`, or `None` when the column holds one
    /// value.
    pub(super) fn axis(&self, column: usize, texts: &BTreeMap<u64, ConfigTexts>) -> Option<ResultsAxis> {
        let mut seen = BTreeSet::new();
        let mut distinct: Vec<&str> = Vec::new();
        for config in texts.values() {
            if let Some(text) = config.texts.get(column)
                && seen.insert(text.as_str())
            {
                distinct.push(text);
            }
        }
        if distinct.len() < 2 {
            return None;
        }
        let numbers: Option<Vec<f64>> = distinct.iter().map(|text| finite_number(text)).collect();
        let (levels, positions, numeric) = if let Some(numbers) = numbers {
            let mut pairs: Vec<(f64, &str)> = numbers.into_iter().zip(distinct).collect();
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let levels = pairs.iter().map(|&(_, text)| text.to_owned()).collect();
            (levels, pairs.iter().map(|&(value, _)| value).collect(), true)
        } else {
            let positions = (0..distinct.len()).map(|level| level as f64).collect();
            (distinct.into_iter().map(str::to_owned).collect(), positions, false)
        };
        Some(ResultsAxis::new(
            column,
            self.column_label(column),
            levels,
            positions,
            numeric,
        ))
    }

    /// Returns the name that the views use for value column `column`:
    /// a parameter's label, or an action's label and "tick".
    ///
    /// An action or a parameter that this device's model does not declare keeps the name from its column.
    pub(super) fn column_label(&self, column: usize) -> String {
        let name = &self.value_columns[column];
        if let Some(action) = name.strip_prefix(ACTION_COLUMN_PREFIX) {
            let label = self.action_labels.get(action).map_or(action, String::as_str);
            return format!("{label} tick");
        }
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id() == name)
            .map_or_else(|| name.clone(), |descriptor| descriptor.label().to_owned())
    }

    /// Adds the configs of the candidates that `updates` report, and each batch in order to the search history.
    ///
    /// A batch that the history already holds, as when a resumed search reports it again, adds no second entry. The
    /// configs of every batch join the configs held in one pass over the axes.
    pub fn push_search_updates<'a>(&mut self, updates: impl IntoIterator<Item = &'a SearchUpdate>) {
        let Some(search) = &mut self.search else {
            return;
        };
        let mut added: BTreeMap<u64, ConfigTexts> = BTreeMap::new();
        for update in updates {
            let known = search
                .history
                .batches
                .last()
                .is_some_and(|last| last.batch >= update.batch);
            if !known {
                search.history.push(update);
            }
            for evaluated in &update.evaluated {
                if !self.configs.contains_key(&evaluated.candidate_id) {
                    added
                        .entry(evaluated.candidate_id)
                        .or_insert_with(|| config_texts(&evaluated.config, &self.descriptors));
                }
            }
        }
        if !added.is_empty() && !self.descriptors.is_empty() {
            self.add_configs(added);
        }
        self.revision += 1;
    }

    /// Adds the configs `added` beside the configs held, and assigns each config its runs.
    ///
    /// The axes grow in place when [`Self::fits_axes`] finds that the added configs fit them. Otherwise every config
    /// is built again and the axes are found again.
    pub(super) fn add_configs(&mut self, added: BTreeMap<u64, ConfigTexts>) {
        if self.fits_axes(&added) {
            self.extend_configs(added);
        } else {
            self.rebuild_configs(added);
        }
    }

    /// Returns whether the configs `added` fit the axes held: every added id comes after every held id, no column
    /// that is not an axis takes a second value, and every value on a numeric axis is a finite number.
    ///
    /// A store that holds no config yet fits nothing.
    pub(super) fn fits_axes(&self, added: &BTreeMap<u64, ConfigTexts>) -> bool {
        let (Some((&first_added, _)), Some((&last_held, _))) = (added.first_key_value(), self.configs.last_key_value())
        else {
            return false;
        };
        if first_added <= last_held {
            return false;
        }
        let added_texts = |column: usize| added.values().filter_map(move |config| config.texts.get(column));
        let numbers_fit = self
            .axes
            .iter()
            .filter(|axis| axis.numeric)
            .all(|axis| added_texts(axis.column).all(|text| finite_number(text).is_some()));
        let axis_columns: BTreeSet<usize> = self.axes.iter().map(|axis| axis.column).collect();
        numbers_fit
            && (0..self.value_columns.len())
                .filter(|column| !axis_columns.contains(column))
                .all(|column| {
                    let held = self.configs.values().find_map(|config| config.texts.get(column));
                    let mut texts = held.into_iter().chain(added_texts(column));
                    let first = texts.next();
                    texts.all(|text| Some(text) == first)
                })
    }

    /// Adds the configs `added`, which fit the axes held, and assigns each added config its runs.
    ///
    /// Every config and block held keeps its entry. A level keeps its id when levels are added below it.
    pub(super) fn extend_configs(&mut self, added: BTreeMap<u64, ConfigTexts>) {
        for axis in &mut self.axes {
            let column = axis.column;
            let texts = added.values().filter_map(|config| config.texts.get(column));
            axis.extend(texts.map(String::as_str));
        }
        let axis_count = self.axes.len();
        for (config_id, ConfigTexts { block, texts }) in added {
            let level_ids: Vec<usize> = self
                .axes
                .iter()
                .map(|axis| texts.get(axis.column).map_or(NO_LEVEL_ID, |text| axis.level_id(text)))
                .collect();
            self.block_axes
                .entry(block)
                .or_insert_with(|| BlockAxes::new(axis_count))
                .add(&level_ids);
            let config = ConfigEntry {
                block,
                texts,
                level_ids,
                runs: self.unassigned_runs.remove(&config_id).unwrap_or_default(),
            };
            self.configs.insert(config_id, config);
        }
    }

    /// Builds every config again from the configs held and `added`, finds the axes again and assigns each config
    /// its runs.
    pub(super) fn rebuild_configs(&mut self, added: BTreeMap<u64, ConfigTexts>) {
        let mut texts: BTreeMap<u64, ConfigTexts> = std::mem::take(&mut self.configs)
            .into_iter()
            .map(|(config_id, config)| {
                let texts = ConfigTexts {
                    block: config.block,
                    texts: config.texts,
                };
                (config_id, texts)
            })
            .collect();
        for (config_id, config) in added {
            texts.entry(config_id).or_insert(config);
        }
        self.set_configs(texts);
        self.unassigned_runs.clear();
        let mut positions: Vec<usize> = (0..self.runs.len()).collect();
        positions.sort_by_key(|&position| self.runs[position].run.run_id);
        for position in positions {
            let config_id = self.runs[position].run.config_id;
            match self.configs.get_mut(&config_id) {
                Some(config) => config.runs.push(position),
                None => self.unassigned_runs.entry(config_id).or_default().push(position),
            }
        }
    }

    pub fn axes(&self) -> &[ResultsAxis] {
        &self.axes
    }

    /// Ids of every config, planned or seen in a run.
    pub fn config_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.configs.keys().copied()
    }

    /// Number of configs, planned or seen in a run.
    pub fn config_count(&self) -> usize {
        self.configs.len()
    }

    /// Returns the level of config `config_id` on axis `axis`.
    pub fn config_level(&self, config_id: u64, axis: usize) -> Option<usize> {
        self.entry_level(self.configs.get(&config_id)?, axis)
    }

    /// Returns the level of `config` on axis `axis`, `None` where the config lacks the axis's column.
    pub(super) fn entry_level(&self, config: &ConfigEntry, axis: usize) -> Option<usize> {
        self.axes.get(axis)?.level_of_id(*config.level_ids.get(axis)?)
    }

    /// Returns the word that the views use for a config, "Candidate" for the results of a search.
    pub fn config_noun(&self) -> &'static str {
        if self.is_search() { "Candidate" } else { "Config" }
    }

    /// Returns the plural that the views use for the configs, "candidates" for the results of a search.
    pub fn configs_noun(&self) -> &'static str {
        if self.is_search() {
            "candidates"
        } else {
            "configurations"
        }
    }

    /// Returns a name for config `config_id` that shows its level on every axis.
    pub fn config_label(&self, config_id: u64) -> String {
        let noun = self.config_noun();
        let Some(config) = self.configs.get(&config_id) else {
            return format!("{noun} {config_id}");
        };
        let levels: Vec<String> = self
            .axes
            .iter()
            .filter_map(|axis| Some(format!("{} {}", axis.label, config.texts.get(axis.column)?)))
            .collect();
        if levels.is_empty() {
            format!("{noun} {config_id}")
        } else {
            format!("{noun} {config_id}: {}", levels.join(", "))
        }
    }

    /// Returns the label and value of each column that config `config_id` varies, the value as `runs.csv` writes it.
    ///
    /// A search's config lists every parameter and tick the search picks, and a sweep's config its level on every
    /// axis. Returns `None` for a config the store does not hold, as a search candidate before its batch ends.
    pub fn config_values(&self, config_id: u64) -> Option<Vec<(String, String)>> {
        let config = self.configs.get(&config_id)?;
        let columns: Vec<usize> = match &self.search {
            Some(search) => search
                .spec
                .space
                .iter()
                .filter_map(|factor| self.target_column(&factor.target))
                .collect(),
            None => self.axes.iter().map(|axis| axis.column).collect(),
        };
        let values = columns
            .into_iter()
            .filter_map(|column| Some((self.column_label(column), config.texts.get(column)?.clone())))
            .collect();
        Some(values)
    }

    /// Returns the values of config `config_id` on one line, as in "Infection Rate 0.2, Recovery Rate 0.05", or
    /// `None` for a config the store does not hold.
    pub fn config_values_text(&self, config_id: u64) -> Option<String> {
        let values = self.config_values(config_id)?;
        let parts: Vec<String> = values.iter().map(|(label, value)| format!("{label} {value}")).collect();
        Some(parts.join(", "))
    }

    /// Unit of the model's time scale, `None` for a model with no time scale or when this device lacks it.
    ///
    /// Note that the unit is `None` too when the value columns hold no tick length, as in a folder written before the
    /// model declared a time scale. The runs of such a folder draw at their ticks.
    pub fn time_unit(&self) -> Option<TimeUnit> {
        self.time
            .as_ref()
            .filter(|time| self.value_columns.iter().any(|column| *column == time.tick_length))
            .map(|time| time.unit)
    }

    /// Returns the unit and tick length that the runs of config `config_id` ran with, `None` for a model with no time
    /// scale, a config the store does not hold, or a tick length missing from the config's values.
    ///
    /// The time at a tick of such a run is the value that its series' `time` column holds.
    pub fn config_time(&self, config_id: u64) -> Option<RunTime> {
        let time = self.time.as_ref()?;
        let column = self.value_columns.iter().position(|name| *name == time.tick_length)?;
        let text = self.configs.get(&config_id)?.texts.get(column)?;
        Some(RunTime {
            unit: time.unit,
            tick_length: text.parse().ok()?,
        })
    }

    /// Returns the value column that `target` sets, `None` for a parameter or action missing from the columns.
    pub(super) fn target_column(&self, target: &FactorTarget) -> Option<usize> {
        let name = match target {
            FactorTarget::Param(id) => id.clone(),
            FactorTarget::Action(name) => format!("{ACTION_COLUMN_PREFIX}{name}"),
        };
        self.value_columns.iter().position(|column| *column == name)
    }
}

/// Returns the names of the value columns of `plan`: each parameter of `params`, then each action's tick.
pub(super) fn plan_value_columns(plan: &Plan, params: &[ParamDescriptor]) -> Vec<String> {
    let actions = plan.actions().iter().map(|action| action.column_name());
    params
        .iter()
        .map(|param| param.id().to_owned())
        .chain(actions)
        .collect()
}

/// Returns the block and values of every config of `plan` by id, as `runs.csv` writes them for the parameters
/// `params`.
pub(super) fn plan_texts(plan: &Plan, params: &[ParamDescriptor]) -> BTreeMap<u64, ConfigTexts> {
    plan.configs()
        .iter()
        .enumerate()
        .map(|(config_id, config)| (config_id as u64, config_texts(config, params)))
        .collect()
}

/// Returns the block and values of `config`, as `runs.csv` writes them for the parameters `params`.
pub(super) fn config_texts(config: &Config, params: &[ParamDescriptor]) -> ConfigTexts {
    let values = params
        .iter()
        .zip(&config.params)
        .map(|(param, value)| format_value(param.kind(), value));
    let ticks = config.action_ticks.iter().map(u64::to_string);
    ConfigTexts {
        block: config.block,
        texts: values.chain(ticks).collect(),
    }
}

/// Returns the levels the configs of each block of `configs` take on `axis_count` axes, by block index.
pub(super) fn block_axes(configs: &BTreeMap<u64, ConfigEntry>, axis_count: usize) -> BTreeMap<usize, BlockAxes> {
    let mut blocks: BTreeMap<usize, BlockAxes> = BTreeMap::new();
    for config in configs.values() {
        blocks
            .entry(config.block)
            .or_insert_with(|| BlockAxes::new(axis_count))
            .add(&config.level_ids);
    }
    blocks
}

/// Returns `text` as a number, or `None` when it is not a finite number.
pub(super) fn finite_number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}
