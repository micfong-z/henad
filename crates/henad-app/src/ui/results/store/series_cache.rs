//! Series of the runs of a results store, held within a byte budget.

use std::collections::{BTreeMap, BTreeSet};

use henad_core::explore::measure::SeriesBuffer;

use super::ResultsStore;

/// Series of runs, held while they fit a byte budget.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesCache {
    /// Maximum number of bytes the cache holds.
    budget: usize,
    /// Size in bytes of the held series.
    used: usize,
    runs: BTreeMap<u64, SeriesBuffer>,
}

impl SeriesCache {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            runs: BTreeMap::new(),
        }
    }

    /// Holds `series` as the series of run `run_id`, and returns whether it fits the budget.
    ///
    /// A series that does not fit is dropped.
    pub fn insert(&mut self, run_id: u64, series: SeriesBuffer) -> bool {
        self.remove(run_id);
        let bytes = series_bytes(&series);
        if self.used + bytes > self.budget {
            return false;
        }
        self.used += bytes;
        self.runs.insert(run_id, series);
        true
    }

    pub fn get(&self, run_id: u64) -> Option<&SeriesBuffer> {
        self.runs.get(&run_id)
    }

    pub(super) fn remove(&mut self, run_id: u64) {
        if let Some(series) = self.runs.remove(&run_id) {
            self.used -= series_bytes(&series);
        }
    }

    /// Drops held series of runs outside `kept_runs`, highest run id first, until `bytes` more fit the budget.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn make_room(&mut self, bytes: usize, kept_runs: &BTreeSet<u64>) {
        let evictable: Vec<u64> = self
            .runs
            .keys()
            .rev()
            .copied()
            .filter(|id| !kept_runs.contains(id))
            .collect();
        for run_id in evictable {
            if self.used + bytes <= self.budget {
                return;
            }
            self.remove(run_id);
        }
    }

    /// Number of runs whose series is held.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// Size in bytes of the held series.
    #[cfg(test)]
    pub fn used_bytes(&self) -> usize {
        self.used
    }
}

/// Returns the size of `series` in bytes, counted the same way as a sweep's events.
pub(super) fn series_bytes(series: &SeriesBuffer) -> usize {
    series.len() * (series.width() + 1) * size_of::<f64>()
}

impl ResultsStore {
    pub fn series(&self, run_id: u64) -> Option<&SeriesBuffer> {
        self.series.get(run_id)
    }

    pub fn series_cache(&self) -> &SeriesCache {
        &self.series
    }

    /// Number of runs that recorded no series rows.
    pub fn empty_series_count(&self) -> usize {
        self.empty_series_runs.len()
    }

    /// Returns the ids of the runs of the configs `config_ids` whose series is not held, leaving out runs that
    /// recorded no series.
    pub fn runs_without_series(&self, config_ids: &BTreeSet<u64>) -> BTreeSet<u64> {
        config_ids
            .iter()
            .flat_map(|&config_id| self.config_runs(config_id))
            .map(|outcome| outcome.run.run_id)
            .filter(|&run_id| self.series.get(run_id).is_none() && !self.empty_series_runs.contains(&run_id))
            .collect()
    }

    /// Records that the runs `run_ids` have no series rows, so [`Self::runs_without_series`] leaves them out.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn record_empty_series(&mut self, run_ids: &BTreeSet<u64>) {
        let known = run_ids.iter().filter(|run_id| self.positions.contains_key(run_id));
        self.empty_series_runs.extend(known);
        self.revision += 1;
    }

    /// Returns the number of bytes of series that a load can add while the series of `kept_runs` stay held.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn series_room(&self, kept_runs: &BTreeSet<u64>) -> usize {
        let kept_bytes: usize = kept_runs
            .iter()
            .filter_map(|&run_id| self.series.get(run_id))
            .map(series_bytes)
            .sum();
        self.series.budget.saturating_sub(kept_bytes)
    }

    /// Holds `series`, series of runs by id, dropping held series of runs outside `kept_runs` to make room, and
    /// returns the number of series that fit.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn insert_series(&mut self, series: BTreeMap<u64, SeriesBuffer>, kept_runs: &BTreeSet<u64>) -> usize {
        let needed = series.values().map(series_bytes).sum();
        self.series.make_room(needed, kept_runs);
        let mut held = 0;
        for (run_id, buffer) in series {
            if self.positions.contains_key(&run_id) && self.series.insert(run_id, buffer) {
                held += 1;
            }
        }
        self.revision += 1;
        held
    }
}
