//! Pattern Space Exploration (PSE), a search for candidates whose outputs land where no candidate has landed yet.
//!
//! A grid of cells spans two output columns. The archive keeps each filled cell with its exemplar, the first
//! candidate to land in it, and counts every candidate that did. After its initial samples, drawn at random, the
//! search draws two filled cells, takes the one with fewer hits, and mutates its exemplar.
//!
//! The initial samples fill batches of their own, and breeding starts once every one of them is told. Without initial
//! samples, the first candidate is drawn at random, alone in its batch. A later batch asked while the archive is still
//! empty is drawn at random in full.
//!
//! An axis given no bounds has an automatic range. The search holds every evaluation until the initial samples are
//! all told, takes the range from their outputs, and then places the held evaluations in candidate order.

use std::collections::BTreeMap;

use crate::explore::design_rng::DesignRng;
use crate::explore::search::genome::{Genome, SearchSpace};
use crate::explore::search::{
    Aggregate, Candidate, CandidateOrigin, CandidateTracker, Evaluation, Proposal, SearchReport, SearchSpecError,
    Searcher, check_setting,
};

/// Fraction of an automatic range's span added below its smallest output and above its largest.
pub const AUTOMATIC_RANGE_MARGIN: f64 = 0.05;

/// One axis of the grid, over one output column.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternAxis {
    /// Output column, a reducer column such as `Infected:max`.
    pub column: String,
    /// Lower bound, or `None` for an automatic range.
    pub min: Option<f64>,
    /// Upper bound, or `None` for an automatic range.
    pub max: Option<f64>,
    /// Number of cells from the lower bound to the upper bound, all of one width.
    pub cells: u32,
}

impl PatternAxis {
    /// Returns an axis over `column` from `min` to `max`, cut into `cells` cells.
    pub fn bounded(column: impl Into<String>, min: f64, max: f64, cells: u32) -> Self {
        Self {
            column: column.into(),
            min: Some(min),
            max: Some(max),
            cells,
        }
    }

    /// Returns an axis over `column` cut into `cells` cells, whose range is taken from the initial samples.
    pub fn automatic(column: impl Into<String>, cells: u32) -> Self {
        Self {
            column: column.into(),
            min: None,
            max: None,
            cells,
        }
    }

    /// Returns the lower and upper bounds, or `None` unless both are given.
    pub fn range(&self) -> Option<(f64, f64)> {
        self.min.zip(self.max)
    }

    /// Returns whether the range is left to the initial samples, with neither bound given.
    pub fn is_automatic(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }

    /// Returns the index of the cell `value` lands in and whether `value` lay outside the axis, or `None` for an
    /// axis without both bounds.
    ///
    /// A value below the lower bound or above the upper bound lands in the edge cell nearer to it, and `NaN` in the
    /// first cell. Each lies outside the axis.
    pub fn cell_index(&self, value: f64) -> Option<(u32, bool)> {
        let (min, max) = self.range()?;
        if value.is_nan() || value < min {
            return Some((0, true));
        }
        if value > max {
            return Some((self.cells - 1, true));
        }
        let fraction = (value - min) / (max - min);
        Some((((fraction * f64::from(self.cells)) as u32).min(self.cells - 1), false))
    }

    /// Returns the lower and upper bounds of cell `index`, or `None` for an axis without both bounds.
    pub fn cell_bounds(&self, index: u32) -> Option<(f64, f64)> {
        let (min, max) = self.range()?;
        let width = (max - min) / f64::from(self.cells);
        let upper = if index + 1 >= self.cells {
            max
        } else {
            min + f64::from(index + 1) * width
        };
        Some((min + f64::from(index) * width, upper))
    }

    /// Checks the bounds and the cells of the axis, naming each by its key in `keys`.
    ///
    /// Both bounds are needed, or neither.
    fn check(&self, keys: [&'static str; 3]) -> Result<(), SearchSpecError> {
        let [min_key, max_key, cells_key] = keys;
        let lone_bound = |missing_key: &'static str, given_key: &str| SearchSpecError::Setting {
            key: missing_key,
            value: "none".to_owned(),
            expected: format!("given with {given_key}, or both left out for an automatic range"),
        };
        match (self.min, self.max) {
            (None, None) => {}
            (Some(min), Some(max)) => {
                check_setting(min.is_finite(), min_key, min, "a finite number")?;
                check_setting(
                    max.is_finite() && max > min,
                    max_key,
                    max,
                    format!("a finite number greater than {min}"),
                )?;
            }
            (Some(_), None) => return Err(lone_bound(max_key, min_key)),
            (None, Some(_)) => return Err(lone_bound(min_key, max_key)),
        }
        check_setting(self.cells >= 1, cells_key, self.cells, "at least 1")
    }
}

/// Returns the range an automatic axis takes from `values`, the outputs the initial samples place on it.
///
/// The range runs from the smallest finite value to the largest, widened at each end by [`AUTOMATIC_RANGE_MARGIN`]
/// of its span. A single value `v` is widened to a span of `max(1, |v|)` around it, and no finite value gives the
/// range 0 to 1.
pub fn automatic_range(values: impl IntoIterator<Item = f64>) -> (f64, f64) {
    let extremes =
        values
            .into_iter()
            .filter(|value| value.is_finite())
            .fold(None, |extremes: Option<(f64, f64)>, value| match extremes {
                None => Some((value, value)),
                Some((low, high)) => Some((low.min(value), high.max(value))),
            });
    let Some((low, high)) = extremes else {
        return (0.0, 1.0);
    };
    if high <= low {
        // A span of 1 alone would put a large value's neighbours outside the axis.
        let half = 0.5 * low.abs().max(1.0);
        return ((low - half).max(f64::MIN), (low + half).min(f64::MAX));
    }
    let margin = (high - low) * AUTOMATIC_RANGE_MARGIN;
    let widened = (low - margin, high + margin);
    if widened.0.is_finite() && widened.1.is_finite() {
        widened
    } else {
        (low, high)
    }
}

/// Settings of a Pattern Space Exploration.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternSpaceSettings {
    pub x_axis: PatternAxis,
    pub y_axis: PatternAxis,
    /// Candidates drawn at random before any is bred from the archive.
    pub initial_samples: u64,
    /// Largest step of a mutated gene, as a fraction of its range.
    pub mutation_scale: f64,
    /// Rule that folds the replicates of a candidate into one value per axis.
    pub aggregate: Aggregate,
}

impl PatternSpaceSettings {
    /// Returns settings over `x_axis` and `y_axis`, with 64 initial samples, a mutation scale of 0.1 and the median.
    pub fn new(x_axis: PatternAxis, y_axis: PatternAxis) -> Self {
        Self {
            x_axis,
            y_axis,
            initial_samples: 64,
            mutation_scale: 0.1,
            aggregate: Aggregate::Median,
        }
    }

    /// Checks both axes, the initial samples and the mutation scale.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError::Setting`] for an axis with no cells, one bound without the other, a bound that is
    /// not finite or a maximum not above the minimum, no initial samples beside an automatic range, or a mutation
    /// scale that is not a positive number.
    pub fn check(&self) -> Result<(), SearchSpecError> {
        self.x_axis
            .check(["pse.x_axis.min", "pse.x_axis.max", "pse.x_axis.cells"])?;
        self.y_axis
            .check(["pse.y_axis.min", "pse.y_axis.max", "pse.y_axis.cells"])?;
        check_setting(
            self.initial_samples >= 1 || self.has_ranges(),
            "pse.initial_samples",
            self.initial_samples,
            "at least 1 for an automatic range",
        )?;
        check_setting(
            self.mutation_scale.is_finite() && self.mutation_scale > 0.0,
            "pse.mutation_scale",
            self.mutation_scale,
            "a positive number",
        )
    }

    /// Returns whether both axes have both bounds.
    pub fn has_ranges(&self) -> bool {
        self.x_axis.range().is_some() && self.y_axis.range().is_some()
    }

    /// Returns the number of initial samples an automatic range is taken from, under a budget of `max_evaluations`.
    pub fn range_sample_count(&self, max_evaluations: u64) -> u64 {
        self.initial_samples.min(max_evaluations)
    }

    /// Returns the number of batches of at most `batch_size` candidates an exploration of `max_evaluations` asks for.
    ///
    /// The initial samples fill batches of their own, and so does the first candidate when there are none.
    pub fn batch_count(&self, max_evaluations: u64, batch_size: usize) -> u64 {
        let batch = (batch_size as u64).max(1);
        let random_samples = self.initial_samples.max(1).min(max_evaluations);
        random_samples.div_ceil(batch) + (max_evaluations - random_samples).div_ceil(batch)
    }

    /// Returns the settings with each automatic axis given the range [`automatic_range`] takes from `outputs`.
    ///
    /// `outputs` holds the x and y output of each initial sample that has both. An axis with bounds keeps them.
    pub fn with_automatic_ranges(&self, outputs: &[(f64, f64)]) -> Self {
        let mut settings = self.clone();
        let axes = [
            (
                &mut settings.x_axis,
                outputs.iter().map(|&(x, _)| x).collect::<Vec<f64>>(),
            ),
            (&mut settings.y_axis, outputs.iter().map(|&(_, y)| y).collect()),
        ];
        for (axis, values) in axes {
            if axis.is_automatic() {
                let (min, max) = automatic_range(values);
                axis.min = Some(min);
                axis.max = Some(max);
            }
        }
        settings
    }

    /// Returns the outputs of `evaluation` on the x and y axes, or `None` when an axis has no finite value.
    ///
    /// Each axis folds the finite values of its column with [`Self::aggregate`], leaving failed replicates out.
    pub fn outputs(&self, evaluation: &Evaluation) -> Option<(f64, f64)> {
        let axis_value = |column: usize| {
            let mut values: Vec<f64> = evaluation
                .outputs
                .iter()
                .filter_map(|row| row.get(column).copied().flatten())
                .filter(|value| value.is_finite())
                .collect();
            self.aggregate.combine(&mut values)
        };
        Some((axis_value(0)?, axis_value(1)?))
    }

    /// Returns where outputs `x` and `y` land, with no cell while an axis lacks its range.
    pub fn locate(&self, x: f64, y: f64) -> PatternPlacement {
        let cell = self.x_axis.cell_index(x).zip(self.y_axis.cell_index(y));
        PatternPlacement {
            x,
            y,
            cell: cell.map(|((x_index, _), (y_index, _))| PatternCell { x_index, y_index }),
            outside: cell.is_some_and(|((_, x_outside), (_, y_outside))| x_outside || y_outside),
        }
    }

    /// Returns where the replicates of `evaluation` land, or `None` when an axis has no finite value.
    ///
    /// The outputs are those [`Self::outputs`] returns, and the placement has no cell while an axis lacks its range.
    pub fn place(&self, evaluation: &Evaluation) -> Option<PatternPlacement> {
        let (x, y) = self.outputs(evaluation)?;
        Some(self.locate(x, y))
    }
}

/// Cell of the grid, by its index along each axis.
///
/// Cells order by `x_index`, then by `y_index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternCell {
    pub x_index: u32,
    pub y_index: u32,
}

/// Outputs of an evaluation, and the cell they land in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternPlacement {
    pub x: f64,
    pub y: f64,
    /// Cell the outputs land in, `None` while an automatic range waits for the initial samples.
    pub cell: Option<PatternCell>,
    /// Whether an output lay outside its axis and was moved into the edge cell.
    pub outside: bool,
}

/// A filled cell of the archive.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveEntry {
    pub cell: PatternCell,
    /// Candidates that landed in the cell.
    pub hits: u64,
    /// Id of the exemplar, the first candidate to land in the cell.
    pub candidate_id: u64,
    /// Output of the exemplar on the x axis.
    pub x: f64,
    /// Output of the exemplar on the y axis.
    pub y: f64,
}

/// Record of a filled cell, its entry with the exemplar's genome.
#[derive(Debug, Clone, PartialEq)]
struct ArchiveRecord {
    entry: ArchiveEntry,
    genome: Genome,
}

/// An evaluation told before its automatic range was taken, with its outputs.
#[derive(Debug, Clone, PartialEq)]
struct HeldEvaluation {
    candidate: Candidate,
    x: f64,
    y: f64,
}

/// A Pattern Space Exploration.
#[derive(Debug, Clone)]
pub struct PatternSpaceExploration {
    space: SearchSpace,
    /// Settings as given, and once the initial samples are told, with each automatic range taken.
    settings: PatternSpaceSettings,
    rng: DesignRng,
    tracker: CandidateTracker,
    archive: BTreeMap<PatternCell, ArchiveRecord>,
    /// Number of initial samples within the budget, the candidates an automatic range is taken from.
    range_samples: u64,
    /// Initial samples told so far.
    told_samples: u64,
    /// Evaluations with outputs on both axes told while an automatic range waits, in candidate order.
    held: Vec<HeldEvaluation>,
}

impl PatternSpaceExploration {
    /// Returns an exploration of `max_evaluations` evaluations over `space`, drawing from `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError`] when [`PatternSpaceSettings::check`] refuses `settings`.
    pub fn new(
        space: SearchSpace,
        settings: PatternSpaceSettings,
        max_evaluations: u64,
        seed: u64,
    ) -> Result<Self, SearchSpecError> {
        settings.check()?;
        Ok(Self {
            space,
            range_samples: settings.range_sample_count(max_evaluations),
            settings,
            rng: DesignRng::new(seed),
            tracker: CandidateTracker::new(max_evaluations),
            archive: BTreeMap::new(),
            told_samples: 0,
            held: Vec::new(),
        })
    }

    /// Files a candidate with outputs `x` and `y` in the cell they land in, as its exemplar when the cell is empty.
    fn file(&mut self, candidate: Candidate, x: f64, y: f64) {
        let Some(cell) = self.settings.locate(x, y).cell else {
            return;
        };
        self.archive
            .entry(cell)
            .or_insert_with(|| ArchiveRecord {
                entry: ArchiveEntry {
                    cell,
                    hits: 0,
                    candidate_id: candidate.id,
                    x,
                    y,
                },
                genome: candidate.genome,
            })
            .entry
            .hits += 1;
    }
}

/// Returns a child of the exemplar of the rarer of two filled cells, each drawn uniformly from `records`.
fn offspring(space: &SearchSpace, rng: &mut DesignRng, mutation_scale: f64, records: &[&ArchiveRecord]) -> Proposal {
    let first = records[rng.index(records.len() as u64) as usize];
    let second = records[rng.index(records.len() as u64) as usize];
    let parent = if second.entry.hits < first.entry.hits {
        second
    } else {
        first
    };
    Proposal::first_evaluation(
        space.mutate(&parent.genome, rng, 1.0, mutation_scale),
        CandidateOrigin::Mutation {
            parent_id: parent.entry.candidate_id,
        },
    )
}

impl Searcher for PatternSpaceExploration {
    fn ask(&mut self, max: usize) -> Vec<Candidate> {
        let mut count = self.tracker.capacity(max) as u64;
        let random_samples = self.range_samples.max(1);
        let sampling = self.tracker.issued < random_samples;
        if sampling {
            count = count.min(random_samples - self.tracker.issued);
        }
        let records: Vec<&ArchiveRecord> = self.archive.values().collect();
        let proposals = (0..count)
            .map(|_| {
                if sampling || records.is_empty() {
                    Proposal::first_evaluation(self.space.random_genome(&mut self.rng), CandidateOrigin::Random)
                } else {
                    offspring(&self.space, &mut self.rng, self.settings.mutation_scale, &records)
                }
            })
            .collect();
        self.tracker.issue(proposals)
    }

    fn tell(&mut self, evaluations: &[Evaluation]) {
        let mut sorted: Vec<&Evaluation> = evaluations.iter().collect();
        sorted.sort_by_key(|evaluation| evaluation.candidate_id);
        let waiting = !self.settings.has_ranges();
        for evaluation in sorted {
            let Some((candidate, _)) = self.tracker.settle(evaluation.candidate_id) else {
                continue;
            };
            if candidate.id < self.range_samples {
                self.told_samples += 1;
            }
            let Some((x, y)) = self.settings.outputs(evaluation) else {
                continue;
            };
            if waiting {
                self.held.push(HeldEvaluation { candidate, x, y });
            } else {
                self.file(candidate, x, y);
            }
        }
        if waiting && self.told_samples >= self.range_samples {
            let samples: Vec<(f64, f64)> = self
                .held
                .iter()
                .filter(|held| held.candidate.id < self.range_samples)
                .map(|held| (held.x, held.y))
                .collect();
            self.settings = self.settings.with_automatic_ranges(&samples);
            for held in std::mem::take(&mut self.held) {
                self.file(held.candidate, held.x, held.y);
            }
        }
    }

    fn is_done(&self) -> bool {
        self.tracker.is_done()
    }

    fn report(&self) -> SearchReport {
        SearchReport {
            archive: self.archive.values().map(|record| record.entry.clone()).collect(),
            ..SearchReport::default()
        }
    }

    fn archive_entry(&self, cell: PatternCell) -> Option<ArchiveEntry> {
        self.archive.get(&cell).map(|record| record.entry.clone())
    }

    fn filled_cells(&self) -> u64 {
        self.archive.len() as u64
    }

    fn pattern_settings(&self) -> Option<&PatternSpaceSettings> {
        self.settings.has_ranges().then_some(&self.settings)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AUTOMATIC_RANGE_MARGIN, PatternAxis, PatternCell, PatternSpaceExploration, PatternSpaceSettings,
        automatic_range,
    };
    use crate::explore::fingerprint::fnv1a64;
    use crate::explore::search::tests::support::{drive, drive_batches, noise, unit_space};
    use crate::explore::search::{Candidate, CandidateOrigin, Evaluation, SearchSpecError, Searcher as _};

    fn axis(column: &str, cells: u32) -> PatternAxis {
        PatternAxis::bounded(column, 0.0, 100.0, cells)
    }

    /// Returns outputs that crowd toward 0 on both axes, as `100 * gene^6`.
    fn skewed(candidate: &Candidate, _: u64) -> Vec<Option<f64>> {
        candidate
            .genome
            .genes()
            .iter()
            .map(|&gene| {
                let cube = gene * gene * gene;
                Some(100.0 * cube * cube)
            })
            .collect()
    }

    /// Returns noisy outputs, with the y output of replicate 1 missing for every seventh candidate.
    fn noisy(candidate: &Candidate, replicate: u64) -> Vec<Option<f64>> {
        let genes = candidate.genome.genes();
        let cube = genes[0] * genes[0] * genes[0];
        vec![
            Some(100.0 * cube * cube + noise(candidate.id, replicate)),
            (replicate != 1 || !candidate.id.is_multiple_of(7))
                .then(|| 80.0 * genes[1] + 30.0 * noise(candidate.id, replicate)),
        ]
    }

    /// Returns the outputs of [`noisy`], with no y output at all for every ninth candidate.
    fn gappy(candidate: &Candidate, replicate: u64) -> Vec<Option<f64>> {
        let mut row = noisy(candidate, replicate);
        if candidate.id % 9 == 4 {
            row[1] = None;
        }
        row
    }

    /// Returns a hash of every candidate `search` asks for, over batches of 16 with 3 replicates of [`noisy`], and of
    /// its archive at the end.
    fn trajectory_hash(search: &mut PatternSpaceExploration) -> u64 {
        let asked = drive(search, 16, 3, noisy);
        let mut bytes = Vec::new();
        for candidate in &asked {
            bytes.extend(candidate.id.to_le_bytes());
            for gene in candidate.genome.genes() {
                bytes.extend(gene.to_bits().to_le_bytes());
            }
            bytes.extend(candidate.origin.parent_ids()[0].unwrap_or(u64::MAX).to_le_bytes());
        }
        for entry in search.report().archive {
            bytes.extend(entry.cell.x_index.to_le_bytes());
            bytes.extend(entry.cell.y_index.to_le_bytes());
            bytes.extend(entry.hits.to_le_bytes());
            bytes.extend(entry.candidate_id.to_le_bytes());
            bytes.extend(entry.x.to_bits().to_le_bytes());
            bytes.extend(entry.y.to_bits().to_le_bytes());
        }
        fnv1a64(&bytes)
    }

    fn filled_cells(initial_samples: u64, seed: u64) -> usize {
        let settings = PatternSpaceSettings {
            initial_samples,
            ..PatternSpaceSettings::new(axis("Infected:max", 20), axis("Infected:argmax", 20))
        };
        let mut search = PatternSpaceExploration::new(unit_space(2), settings, 400, seed).expect("valid settings");
        drive(&mut search, 16, 1, skewed);
        search.report().archive.len()
    }

    #[test]
    fn pse_fills_more_cells_than_random_search() {
        for seed in 1..=3 {
            // With every sample random, the exploration is a random search of the same budget.
            let random = filled_cells(400, seed);
            let explored = filled_cells(48, seed);
            assert!(
                explored > random,
                "seed {seed}: {explored} cells explored, {random} at random"
            );
        }
    }

    #[test]
    fn explicit_bounds_keep_their_trajectory() {
        // Hashes of the trajectories an exploration with both bounds given takes.
        for (seed, expected) in [(3, 0x2b4e_69fa_34f1_1ba3_u64), (11, 0xccf5_0d16_6148_66ed)] {
            let settings = PatternSpaceSettings {
                initial_samples: 40,
                ..PatternSpaceSettings::new(
                    PatternAxis::bounded("Infected:max", 0.0, 90.0, 20),
                    PatternAxis::bounded("Infected:argmax", 0.0, 70.0, 12),
                )
            };
            let mut search = PatternSpaceExploration::new(unit_space(2), settings.clone(), 300, seed).expect("valid");
            assert_eq!(trajectory_hash(&mut search), expected, "seed {seed}");
            assert_eq!(search.pattern_settings(), Some(&settings), "bounds given are kept");
        }
    }

    /// Returns the size of each batch in `batches`.
    fn sizes(batches: &[Vec<Candidate>]) -> Vec<usize> {
        batches.iter().map(Vec::len).collect()
    }

    fn is_mutation(candidate: &Candidate) -> bool {
        matches!(candidate.origin, CandidateOrigin::Mutation { .. })
    }

    #[test]
    fn a_budget_within_one_batch_breeds_after_the_initial_samples() {
        for (initial_samples, max_evaluations, expected) in [(1, 2, [1, 1]), (3, 8, [3, 5])] {
            let settings = PatternSpaceSettings {
                initial_samples,
                ..PatternSpaceSettings::new(axis("Infected:max", 10), axis("Infected:argmax", 10))
            };
            let mut search =
                PatternSpaceExploration::new(unit_space(2), settings, max_evaluations, 5).expect("valid settings");
            let batches = drive_batches(&mut search, 16, 1, skewed);
            assert_eq!(sizes(&batches), expected, "the initial samples alone, then the rest");
            assert!(
                batches[0]
                    .iter()
                    .all(|candidate| candidate.origin == CandidateOrigin::Random)
            );
            assert!(
                batches[1].iter().all(is_mutation),
                "every candidate after the initial samples comes from the archive"
            );
        }
    }

    #[test]
    fn one_automatic_axis_takes_its_range_and_the_other_keeps_its_bounds() {
        let settings = PatternSpaceSettings {
            initial_samples: 10,
            ..PatternSpaceSettings::new(
                PatternAxis::automatic("Infected:max", 8),
                PatternAxis::bounded("Infected:argmax", 0.0, 50.0, 5),
            )
        };
        let mut search = PatternSpaceExploration::new(unit_space(2), settings.clone(), 60, 3).expect("valid settings");
        let batches = drive_batches(&mut search, 4, 1, skewed);
        assert_eq!(
            sizes(&batches)[..4],
            [4, 4, 2, 4],
            "the third batch ends at the last initial sample"
        );
        let asked = batches.concat();
        for candidate in &asked {
            assert_eq!(
                candidate.origin == CandidateOrigin::Random,
                candidate.id < 10,
                "candidate {}",
                candidate.id
            );
        }
        let resolved = search.pattern_settings().expect("every initial sample told");
        let x_values = asked[..10]
            .iter()
            .map(|candidate| skewed(candidate, 0)[0].expect("a finite output"));
        assert_eq!(resolved.x_axis.range(), Some(automatic_range(x_values)));
        assert_eq!(resolved.y_axis, settings.y_axis, "the bounded axis keeps its bounds");
        assert!(search.filled_cells() > 1, "{} cells", search.filled_cells());
    }

    #[test]
    fn without_initial_samples_the_first_candidate_fills_a_batch_alone() {
        let settings = PatternSpaceSettings {
            initial_samples: 0,
            ..PatternSpaceSettings::new(axis("Infected:max", 10), axis("Infected:argmax", 10))
        };
        let mut search = PatternSpaceExploration::new(unit_space(2), settings.clone(), 20, 7)
            .expect("bounded axes need no initial sample");
        let batches = drive_batches(&mut search, 8, 1, skewed);
        assert_eq!(sizes(&batches), [1, 8, 8, 3]);
        assert_eq!(batches[0][0].origin, CandidateOrigin::Random);
        assert!(
            batches[1..].iter().flatten().all(is_mutation),
            "the first candidate fills the archive"
        );

        // With no output on the y axis, no candidate lands in a cell.
        let mut search = PatternSpaceExploration::new(unit_space(2), settings, 13, 7).expect("valid settings");
        let missing = |_: &Candidate, _| vec![Some(1.0), None];
        let batches = drive_batches(&mut search, 8, 1, missing);
        assert_eq!(sizes(&batches), [1, 8, 4], "an empty archive asks for whole batches");
        assert!(
            batches
                .iter()
                .flatten()
                .all(|candidate| candidate.origin == CandidateOrigin::Random)
        );
    }

    #[test]
    fn the_batch_count_matches_the_batches_asked_for() {
        let missing = |_: &Candidate, _| vec![None, Some(1.0)];
        let bounded = |initial_samples| PatternSpaceSettings {
            initial_samples,
            ..PatternSpaceSettings::new(axis("Infected:max", 10), axis("Infected:argmax", 10))
        };
        let automatic = |initial_samples| PatternSpaceSettings {
            initial_samples,
            ..automatic_settings()
        };
        for (settings, max_evaluations, batch_size, landing) in [
            (bounded(0), 20, 8, true),
            (bounded(0), 1, 8, true),
            (bounded(3), 8, 16, true),
            (bounded(10), 30, 6, true),
            (bounded(12), 30, 6, true),
            (bounded(50), 30, 6, true),
            (bounded(10), 30, 6, false),
            (automatic(10), 30, 4, true),
            (automatic(10), 30, 4, false),
            (automatic(1), 5, 1, true),
        ] {
            let mut search = PatternSpaceExploration::new(unit_space(2), settings.clone(), max_evaluations, 9)
                .expect("valid settings");
            let batches = if landing {
                drive_batches(&mut search, batch_size, 1, skewed)
            } else {
                drive_batches(&mut search, batch_size, 1, missing)
            };
            assert_eq!(
                settings.batch_count(max_evaluations, batch_size),
                batches.len() as u64,
                "{} initial samples, {max_evaluations} evaluations in batches of {batch_size}, sizes {:?}",
                settings.initial_samples,
                sizes(&batches)
            );
        }
        assert_eq!(bounded(4).batch_count(0, 8), 0);
    }

    /// Returns the settings of an exploration with automatic ranges on both axes and 40 initial samples.
    fn automatic_settings() -> PatternSpaceSettings {
        PatternSpaceSettings {
            initial_samples: 40,
            ..PatternSpaceSettings::new(
                PatternAxis::automatic("Infected:max", 20),
                PatternAxis::automatic("Infected:argmax", 12),
            )
        }
    }

    #[test]
    fn an_automatic_range_spans_the_initial_samples_with_a_margin() {
        let settings = automatic_settings();
        let mut search = PatternSpaceExploration::new(unit_space(2), settings.clone(), 300, 3).expect("valid");
        let mut asked = Vec::new();
        while asked.len() < 32 {
            let batch = search.ask(16);
            let evaluations: Vec<Evaluation> = batch
                .iter()
                .map(|candidate| Evaluation {
                    candidate_id: candidate.id,
                    outputs: (0..3).map(|replicate| gappy(candidate, replicate)).collect(),
                })
                .collect();
            search.tell(&evaluations);
            asked.extend(batch);
        }
        assert_eq!(search.pattern_settings(), None, "32 of 40 initial samples told");
        assert_eq!(search.filled_cells(), 0, "the archive waits for the range");

        let batch = search.ask(16);
        assert_eq!(batch.len(), 8, "the third batch holds the last 8 initial samples alone");
        let evaluations: Vec<Evaluation> = batch
            .iter()
            .map(|candidate| Evaluation {
                candidate_id: candidate.id,
                outputs: (0..3).map(|replicate| gappy(candidate, replicate)).collect(),
            })
            .collect();
        search.tell(&evaluations);
        asked.extend(batch);
        let resolved = search.pattern_settings().expect("every initial sample told").clone();

        let outputs: Vec<(f64, f64)> = asked[..40]
            .iter()
            .filter_map(|candidate| {
                settings.outputs(&Evaluation {
                    candidate_id: candidate.id,
                    outputs: (0..3).map(|replicate| gappy(candidate, replicate)).collect(),
                })
            })
            .collect();
        assert!(outputs.len() < 40, "a candidate with no y output is left out");
        for (axis, values) in [
            (&resolved.x_axis, outputs.iter().map(|&(x, _)| x).collect::<Vec<f64>>()),
            (&resolved.y_axis, outputs.iter().map(|&(_, y)| y).collect()),
        ] {
            let low = values.iter().copied().fold(f64::INFINITY, f64::min);
            let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let margin = (high - low) * AUTOMATIC_RANGE_MARGIN;
            assert_eq!(axis.range(), Some((low - margin, high + margin)), "{}", axis.column);
        }
        assert_eq!(
            (resolved.x_axis.cells, resolved.y_axis.cells, resolved.initial_samples),
            (20, 12, 40)
        );

        // Every candidate told so far is placed with the range, the exemplar of a cell being the first to land in it.
        let report = search.report();
        let mut hits = 0;
        for (position, candidate) in asked.iter().enumerate() {
            let evaluation = Evaluation {
                candidate_id: candidate.id,
                outputs: (0..3).map(|replicate| gappy(candidate, replicate)).collect(),
            };
            let Some(cell) = resolved.place(&evaluation).and_then(|placement| placement.cell) else {
                continue;
            };
            hits += 1;
            let entry = report.archive_entry(cell).expect("the cell is filled");
            let first = asked[..position].iter().all(|earlier| {
                resolved
                    .place(&Evaluation {
                        candidate_id: earlier.id,
                        outputs: (0..3).map(|replicate| gappy(earlier, replicate)).collect(),
                    })
                    .and_then(|placement| placement.cell)
                    != Some(cell)
            });
            assert_eq!(entry.candidate_id == candidate.id, first, "candidate {}", candidate.id);
        }
        assert_eq!(report.archive.iter().map(|entry| entry.hits).sum::<u64>(), hits);
    }

    #[test]
    fn an_automatic_range_is_reproducible_from_the_seed() {
        let run = |seed| {
            let mut search =
                PatternSpaceExploration::new(unit_space(2), automatic_settings(), 300, seed).expect("valid settings");
            let hash = trajectory_hash(&mut search);
            (hash, search.pattern_settings().cloned())
        };
        let (hash, settings) = run(3);
        assert_eq!(
            run(3),
            (hash, settings.clone()),
            "one seed, one trajectory and one range"
        );
        let (other_hash, other_settings) = run(11);
        assert_ne!(hash, other_hash);
        assert_ne!(settings, other_settings, "another seed draws other initial samples");
        assert!(settings.is_some_and(|settings| settings.has_ranges()));
    }

    #[test]
    fn an_automatic_range_widens_a_single_value_and_falls_back_without_one() {
        assert_eq!(automatic_range([0.25, f64::NAN, 0.25]), (-0.25, 0.75));
        assert_eq!(automatic_range([-4.0]), (-6.0, -2.0), "a span as wide as the value");
        assert_eq!(
            automatic_range([f64::MAX]),
            (f64::MAX / 2.0, f64::MAX),
            "a finite range"
        );
        assert_eq!(automatic_range([f64::INFINITY]), (0.0, 1.0));
        assert_eq!(automatic_range([]), (0.0, 1.0));
        assert_eq!(automatic_range([10.0, 30.0, 20.0]), (9.0, 31.0));
    }

    #[test]
    fn an_axis_takes_both_bounds_or_neither() {
        let check = |x_axis: PatternAxis, initial_samples| {
            PatternSpaceSettings {
                initial_samples,
                ..PatternSpaceSettings::new(x_axis, PatternAxis::automatic("Infected:argmax", 4))
            }
            .check()
        };
        assert_eq!(check(PatternAxis::automatic("Infected:max", 4), 8), Ok(()));
        assert_eq!(check(axis("Infected:max", 4), 8), Ok(()));
        let lone_min = PatternAxis {
            max: None,
            ..axis("Infected:max", 4)
        };
        assert!(matches!(
            check(lone_min, 8),
            Err(SearchSpecError::Setting {
                key: "pse.x_axis.max",
                ..
            })
        ));
        assert!(
            matches!(
                check(PatternAxis::automatic("Infected:max", 4), 0),
                Err(SearchSpecError::Setting {
                    key: "pse.initial_samples",
                    ..
                })
            ),
            "an automatic range needs an initial sample"
        );
    }

    #[test]
    fn pse_archive_iterates_in_cell_order() {
        let settings = PatternSpaceSettings {
            initial_samples: 20,
            ..PatternSpaceSettings::new(axis("Infected:max", 8), axis("Infected:argmax", 8))
        };
        let mut search = PatternSpaceExploration::new(unit_space(2), settings, 200, 9).expect("valid settings");
        let asked = drive(&mut search, 10, 2, skewed);
        let report = search.report();
        assert!(report.archive.len() > 1, "{} cells", report.archive.len());
        let cells: Vec<PatternCell> = report.archive.iter().map(|entry| entry.cell).collect();
        let mut sorted = cells.clone();
        sorted.sort();
        assert_eq!(cells, sorted);
        assert_eq!(report.archive.iter().map(|entry| entry.hits).sum::<u64>(), 200);
        for entry in &report.archive {
            assert_eq!(report.archive_entry(entry.cell), Some(entry));
            let exemplar = &asked[entry.candidate_id as usize];
            let placed = search
                .settings
                .place(&Evaluation {
                    candidate_id: exemplar.id,
                    outputs: vec![skewed(exemplar, 0)],
                })
                .expect("finite outputs");
            assert_eq!(placed.cell, Some(entry.cell), "the exemplar lands in its own cell");
            let earlier = asked[..entry.candidate_id as usize].iter().any(|candidate| {
                search
                    .settings
                    .place(&Evaluation {
                        candidate_id: candidate.id,
                        outputs: vec![skewed(candidate, 0)],
                    })
                    .is_some_and(|placement| placement.cell == Some(entry.cell))
            });
            assert!(!earlier, "no earlier candidate landed in cell {:?}", entry.cell);
        }
    }

    #[test]
    fn a_value_outside_an_axis_lands_in_the_edge_cell() {
        let axis = axis("Infected:max", 4);
        assert_eq!(axis.cell_index(-3.0), Some((0, true)));
        assert_eq!(axis.cell_index(0.0), Some((0, false)));
        assert_eq!(axis.cell_index(24.9), Some((0, false)));
        assert_eq!(axis.cell_index(25.0), Some((1, false)));
        assert_eq!(axis.cell_index(100.0), Some((3, false)));
        assert_eq!(axis.cell_index(250.0), Some((3, true)));
        assert_eq!(axis.cell_index(f64::NAN), Some((0, true)), "NaN lies outside the axis");
        assert_eq!(axis.cell_index(f64::NEG_INFINITY), Some((0, true)));
        assert_eq!(axis.cell_index(f64::INFINITY), Some((3, true)));
        assert_eq!(axis.cell_bounds(1), Some((25.0, 50.0)));
        assert_eq!(axis.cell_bounds(3), Some((75.0, 100.0)));
        let automatic = PatternAxis::automatic("Infected:max", 4);
        assert_eq!((automatic.cell_index(3.0), automatic.cell_bounds(0)), (None, None));
    }

    #[test]
    fn a_replicate_without_a_finite_value_is_left_out_of_its_axis() {
        let settings = PatternSpaceSettings::new(axis("Infected:max", 10), axis("Infected:argmax", 10));
        let evaluation = |outputs| Evaluation {
            candidate_id: 0,
            outputs,
        };
        let placed = settings
            .place(&evaluation(vec![
                vec![Some(15.0), None],
                vec![None, Some(95.0)],
                vec![Some(f64::NAN), Some(96.0)],
            ]))
            .expect("each axis has a finite value");
        assert_eq!((placed.x, placed.y), (15.0, 95.5));
        assert_eq!(placed.cell, Some(PatternCell { x_index: 1, y_index: 9 }));
        assert!(settings.place(&evaluation(vec![vec![None, Some(1.0)]])).is_none());
    }
}
