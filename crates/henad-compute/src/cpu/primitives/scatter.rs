//! Combines per-agent deposits into a grid, for models where many agents write the same cell.
//!
//! Two arms behind one API, picked from how much scratch the shadow route would need. That depends
//! on the worker count, so both arms have to produce identical bits.

use rayon::prelude::*;

/// Shadow scratch above this falls back to sorting.
pub const SHADOW_BUDGET_BYTES: usize = 256 << 20;

/// Rule for combining deposits that land in the same cell.
///
/// Both are commutative and associative, which is what lets the scatter run in parallel at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Combine {
    /// Values must be non-negative, since `0.0` is the identity.
    Max,
    /// Fixed point at `scale` steps per unit, because f32 addition is not associative.
    SumFixed { scale: f32 },
}

impl Combine {
    fn shadow_elem_bytes(self) -> usize {
        match self {
            Self::Max => size_of::<f32>(),
            Self::SumFixed { .. } => size_of::<u64>(),
        }
    }
}

/// The arm a [`ScatterGrid`] resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Shadow,
    Sorted,
}

/// Reusable scratch for combining per-agent deposits into a grid.
///
/// Only the chosen arm's buffers get allocated, the other arm's stay empty.
pub struct ScatterGrid {
    n_cells: usize,
    combine: Combine,
    strategy: Strategy,

    // Shadow arm. One entry per worker, of whichever width `combine` needs.
    shadow_max: Vec<Vec<f32>>,
    shadow_sum: Vec<Vec<u64>>,

    // Sorted arm.
    cell_start: Vec<u32>,
    /// Retained rather than cloned from `cell_start` per call, so a scatter does not allocate.
    write_pos: Vec<u32>,
    sorted_values: Vec<f32>,
}

impl ScatterGrid {
    pub fn new(n_cells: usize, combine: Combine) -> Self {
        Self::with_budget(n_cells, combine, SHADOW_BUDGET_BYTES)
    }

    /// Explicit budget, so a test can pin either arm and check they agree.
    pub fn with_budget(n_cells: usize, combine: Combine, budget_bytes: usize) -> Self {
        let workers = worker_count();
        let shadow_bytes = n_cells
            .saturating_mul(combine.shadow_elem_bytes())
            .saturating_mul(workers);

        let mut grid = Self {
            n_cells,
            combine,
            strategy: if shadow_bytes <= budget_bytes {
                Strategy::Shadow
            } else {
                Strategy::Sorted
            },
            shadow_max: Vec::new(),
            shadow_sum: Vec::new(),
            cell_start: Vec::new(),
            write_pos: Vec::new(),
            sorted_values: Vec::new(),
        };

        match (grid.strategy, combine) {
            (Strategy::Shadow, Combine::Max) => grid.shadow_max = vec![vec![0.0; n_cells]; workers],
            (Strategy::Shadow, Combine::SumFixed { .. }) => grid.shadow_sum = vec![vec![0; n_cells]; workers],
            (Strategy::Sorted, _) => {
                grid.cell_start = vec![0; n_cells + 1];
                grid.write_pos = vec![0; n_cells + 1];
            }
        }
        grid
    }

    pub fn strategy(&self) -> Strategy {
        self.strategy
    }

    pub fn combine(&self) -> Combine {
        self.combine
    }

    /// Writes `out[c] = combine(base[c], every value whose cell is c)`.
    ///
    /// Lanes rather than a per-call deposit, since the sorted arm needs the whole mapping up front.
    /// Depositing the identity is a no-op, so a model with two fields can keep one dense lane set.
    pub fn scatter(&mut self, cells: &[u32], values: &[f32], base: &[f32], out: &mut [f32]) {
        assert_eq!(
            cells.len(),
            values.len(),
            "cell and value lanes must be the same length"
        );
        assert_eq!(base.len(), self.n_cells, "base grid is not this scatter grid's size");
        assert_eq!(out.len(), self.n_cells, "output grid is not this scatter grid's size");

        match self.strategy {
            Strategy::Shadow => self.scatter_shadow(cells, values, base, out),
            Strategy::Sorted => self.scatter_sorted(cells, values, base, out),
        }
    }

    /// Approximate heap bytes owned by the scratch, for a model's `heap_bytes`.
    pub fn heap_bytes(&self) -> usize {
        self.shadow_max
            .iter()
            .map(|s| s.capacity() * size_of::<f32>())
            .sum::<usize>()
            + self
                .shadow_sum
                .iter()
                .map(|s| s.capacity() * size_of::<u64>())
                .sum::<usize>()
            + (self.cell_start.capacity() + self.write_pos.capacity()) * size_of::<u32>()
            + self.sorted_values.capacity() * size_of::<f32>()
    }
}

// Shared inner loops. The parallel and wasm drivers differ only in how they iterate, so all the
// arithmetic lives here and the two cannot drift apart.

#[inline]
fn shadow_chunk_max(shadow: &mut [f32], cells: &[u32], values: &[f32]) {
    shadow.fill(0.0);
    for (&c, &v) in cells.iter().zip(values) {
        debug_assert!(
            v >= 0.0,
            "Combine::Max needs non-negative deposits, {v} would be dropped as below the identity"
        );
        let slot = &mut shadow[c as usize];
        if v > *slot {
            *slot = v;
        }
    }
}

#[inline]
fn shadow_chunk_sum(shadow: &mut [u64], cells: &[u32], values: &[f32], scale: f32) {
    shadow.fill(0);
    for (&c, &v) in cells.iter().zip(values) {
        debug_assert!(v >= 0.0, "Combine::SumFixed needs non-negative deposits, got {v}");
        shadow[c as usize] += fixed(v, scale);
    }
}

#[inline]
fn reduce_shadow_max(c: usize, base: &[f32], shadows: &[Vec<f32>]) -> f32 {
    shadows.iter().fold(base[c], |m, s| m.max(s[c]))
}

#[inline]
fn reduce_shadow_sum(c: usize, base: &[f32], shadows: &[Vec<u64>], scale: f32) -> f32 {
    // Totalled in fixed point before touching f32, so the grouping across shadows cannot matter.
    let total: u64 = shadows.iter().map(|s| s[c]).sum();
    base[c] + unfixed(total, scale)
}

#[inline]
fn reduce_run_max(c: usize, base: &[f32], run: &[f32]) -> f32 {
    run.iter().fold(base[c], |m, &v| m.max(v))
}

#[inline]
fn reduce_run_sum(c: usize, base: &[f32], run: &[f32], scale: f32) -> f32 {
    let total: u64 = run.iter().map(|&v| fixed(v, scale)).sum();
    base[c] + unfixed(total, scale)
}

/// Float to int casts saturate, so a negative deposit floors at 0 instead of wrapping.
#[inline]
fn fixed(v: f32, scale: f32) -> u64 {
    (v * scale) as u64
}

#[inline]
fn unfixed(total: u64, scale: f32) -> f32 {
    total as f32 / scale
}

/// Chunk length that hands each worker exactly one contiguous run of agents.
#[inline]
fn chunk_len(n_agents: usize, workers: usize) -> usize {
    n_agents.div_ceil(workers.max(1)).max(1)
}

fn worker_count() -> usize {
    rayon::current_num_threads()
}

impl ScatterGrid {
    fn scatter_shadow(&mut self, cells: &[u32], values: &[f32], base: &[f32], out: &mut [f32]) {
        let workers = self.shadow_max.len().max(self.shadow_sum.len());
        let chunk = chunk_len(cells.len(), workers);
        // A shadow that got no chunk was never cleared this call, so it must not be reduced.
        let live = cells.len().div_ceil(chunk).min(workers);

        match self.combine {
            Combine::Max => {
                self.shadow_max
                    .par_iter_mut()
                    .zip(cells.par_chunks(chunk))
                    .zip(values.par_chunks(chunk))
                    .for_each(|((shadow, cs), vs)| shadow_chunk_max(shadow, cs, vs));

                let shadows = &self.shadow_max[..live];
                out.par_iter_mut()
                    .enumerate()
                    .for_each(|(c, o)| *o = reduce_shadow_max(c, base, shadows));
            }
            Combine::SumFixed { scale } => {
                self.shadow_sum
                    .par_iter_mut()
                    .zip(cells.par_chunks(chunk))
                    .zip(values.par_chunks(chunk))
                    .for_each(|((shadow, cs), vs)| shadow_chunk_sum(shadow, cs, vs, scale));

                let shadows = &self.shadow_sum[..live];
                out.par_iter_mut()
                    .enumerate()
                    .for_each(|(c, o)| *o = reduce_shadow_sum(c, base, shadows, scale));
            }
        }
    }

    /// Counting sort by cell, permuting the values so the reduce reads contiguously.
    ///
    /// Sequential. The count and permute passes are scatter writes themselves.
    fn build_runs(&mut self, cells: &[u32], values: &[f32]) {
        self.sorted_values.clear();
        self.sorted_values.resize(cells.len(), 0.0);
        self.cell_start.fill(0);

        for &c in cells {
            self.cell_start[c as usize + 1] += 1;
        }
        for i in 1..=self.n_cells {
            self.cell_start[i] += self.cell_start[i - 1];
        }
        self.write_pos.copy_from_slice(&self.cell_start);
        for (i, &c) in cells.iter().enumerate() {
            let p = self.write_pos[c as usize];
            self.sorted_values[p as usize] = values[i];
            self.write_pos[c as usize] = p + 1;
        }
    }

    fn scatter_sorted(&mut self, cells: &[u32], values: &[f32], base: &[f32], out: &mut [f32]) {
        self.build_runs(cells, values);
        let (starts, sorted) = (&self.cell_start, &self.sorted_values);
        let run = |c: usize| &sorted[starts[c] as usize..starts[c + 1] as usize];

        match self.combine {
            Combine::Max => out
                .par_iter_mut()
                .enumerate()
                .for_each(|(c, o)| *o = reduce_run_max(c, base, run(c))),
            Combine::SumFixed { scale } => out
                .par_iter_mut()
                .enumerate()
                .for_each(|(c, o)| *o = reduce_run_sum(c, base, run(c), scale)),
        }
    }
}

#[cfg(test)]
mod tests {
    use henad_core::authoring::primitives::rng::xorshift64;

    use super::*;

    /// Deterministic non-negative workload with genuine collisions.
    fn workload(n_agents: usize, n_cells: usize, mut seed: u64) -> (Vec<u32>, Vec<f32>, Vec<f32>) {
        let mut next = move || {
            seed = xorshift64(seed);
            seed
        };
        let cells = (0..n_agents).map(|_| (next() as usize % n_cells) as u32).collect();
        let values = (0..n_agents).map(|_| (next() >> 40) as f32 / 16_777_216.0).collect();
        let base = (0..n_cells).map(|_| (next() >> 40) as f32 / 16_777_216.0).collect();
        (cells, values, base)
    }

    /// Written the obvious way rather than the fast way.
    fn reference(combine: Combine, cells: &[u32], values: &[f32], base: &[f32], n_cells: usize) -> Vec<f32> {
        let mut out = base.to_vec();
        match combine {
            Combine::Max => {
                for (&c, &v) in cells.iter().zip(values) {
                    out[c as usize] = out[c as usize].max(v);
                }
            }
            Combine::SumFixed { scale } => {
                let mut totals = vec![0u64; n_cells];
                for (&c, &v) in cells.iter().zip(values) {
                    totals[c as usize] += fixed(v, scale);
                }
                for (i, o) in out.iter_mut().enumerate() {
                    *o = base[i] + unfixed(totals[i], scale);
                }
            }
        }
        out
    }

    fn assert_bits_eq(expected: &[f32], got: &[f32], what: &str) {
        let bad = expected.iter().zip(got).position(|(a, b)| a.to_bits() != b.to_bits());
        assert!(bad.is_none(), "{what} disagrees at cell {bad:?}");
    }

    const SUM: Combine = Combine::SumFixed { scale: 1024.0 };

    /// The arm comes from the worker count, so disagreeing arms would make results machine
    /// dependent.
    #[test]
    fn both_arms_agree_bit_for_bit() {
        for combine in [Combine::Max, SUM] {
            let (n_agents, n_cells) = (20_000, 256);
            let (cells, values, base) = workload(n_agents, n_cells, 0x5CA7_7E51_0BEE_F001);

            let mut shadow = ScatterGrid::with_budget(n_cells, combine, usize::MAX);
            let mut sorted = ScatterGrid::with_budget(n_cells, combine, 0);
            assert_eq!(shadow.strategy(), Strategy::Shadow);
            assert_eq!(sorted.strategy(), Strategy::Sorted);

            let mut a = vec![0.0; n_cells];
            let mut b = vec![0.0; n_cells];
            shadow.scatter(&cells, &values, &base, &mut a);
            sorted.scatter(&cells, &values, &base, &mut b);
            assert_bits_eq(&a, &b, "shadow vs sorted");
        }
    }

    #[test]
    fn both_arms_match_the_serial_reference() {
        for combine in [Combine::Max, SUM] {
            let (n_agents, n_cells) = (20_000, 256);
            let (cells, values, base) = workload(n_agents, n_cells, 0x5CA7_7E51_0BEE_F002);
            let expected = reference(combine, &cells, &values, &base, n_cells);

            for budget in [usize::MAX, 0] {
                let mut grid = ScatterGrid::with_budget(n_cells, combine, budget);
                let mut out = vec![0.0; n_cells];
                grid.scatter(&cells, &values, &base, &mut out);
                assert_bits_eq(&expected, &out, &format!("{:?} arm", grid.strategy()));
            }
        }
    }

    /// Scratch has to be consumed, not accumulated.
    #[test]
    fn repeated_scatters_do_not_accumulate() {
        for combine in [Combine::Max, SUM] {
            for budget in [usize::MAX, 0] {
                let (n_agents, n_cells) = (5_000, 128);
                let (cells, values, base) = workload(n_agents, n_cells, 0x5CA7_7E51_0BEE_F003);
                let mut grid = ScatterGrid::with_budget(n_cells, combine, budget);

                let mut first = vec![0.0; n_cells];
                grid.scatter(&cells, &values, &base, &mut first);
                let mut second = vec![0.0; n_cells];
                grid.scatter(&cells, &values, &base, &mut second);
                assert_bits_eq(&first, &second, "second scatter");
            }
        }
    }

    /// A shrinking population must not leave stale shadows in the reduce.
    #[test]
    fn output_is_stable_across_population_changes() {
        for combine in [Combine::Max, SUM] {
            let n_cells = 512;
            let (cells, values, base) = workload(100_000, n_cells, 0x5CA7_7E51_0BEE_F004);
            let mut grid = ScatterGrid::with_budget(n_cells, combine, usize::MAX);

            let mut big = vec![0.0; n_cells];
            grid.scatter(&cells, &values, &base, &mut big);

            // Shrink hard enough that fewer chunks exist than there are shadows.
            let mut small = vec![0.0; n_cells];
            grid.scatter(&cells[..3], &values[..3], &base, &mut small);
            let expected = reference(combine, &cells[..3], &values[..3], &base, n_cells);
            assert_bits_eq(&expected, &small, "shrunken population");

            // Growing back must not be contaminated by either.
            let mut again = vec![0.0; n_cells];
            grid.scatter(&cells, &values, &base, &mut again);
            assert_bits_eq(&big, &again, "regrown population");
        }
    }

    /// A multi-field model relies on this to keep one dense lane set.
    #[test]
    fn identity_deposits_are_a_no_op() {
        for combine in [Combine::Max, SUM] {
            for budget in [usize::MAX, 0] {
                let n_cells = 64;
                let base: Vec<f32> = (0..n_cells).map(|i| i as f32 * 0.25).collect();
                let cells: Vec<u32> = (0..n_cells as u32).collect();
                let values = vec![0.0f32; n_cells];

                let mut grid = ScatterGrid::with_budget(n_cells, combine, budget);
                let mut out = vec![0.0; n_cells];
                grid.scatter(&cells, &values, &base, &mut out);
                assert_bits_eq(&base, &out, "identity deposit");
            }
        }
    }

    #[test]
    fn empty_population_passes_the_base_through() {
        for combine in [Combine::Max, SUM] {
            for budget in [usize::MAX, 0] {
                let n_cells = 32;
                let base: Vec<f32> = (0..n_cells).map(|i| i as f32).collect();
                let mut grid = ScatterGrid::with_budget(n_cells, combine, budget);
                let mut out = vec![0.0; n_cells];
                grid.scatter(&[], &[], &base, &mut out);
                assert_bits_eq(&base, &out, "empty population");
            }
        }
    }

    #[test]
    fn budget_selects_the_arm() {
        let n_cells = 1 << 20;
        let workers = worker_count();
        let just_enough = n_cells * size_of::<f32>() * workers;
        assert_eq!(
            ScatterGrid::with_budget(n_cells, Combine::Max, just_enough).strategy(),
            Strategy::Shadow
        );
        assert_eq!(
            ScatterGrid::with_budget(n_cells, Combine::Max, just_enough - 1).strategy(),
            Strategy::Sorted
        );
    }

    #[test]
    fn fixed_point_round_trip_is_exact_on_multiples_of_the_scale() {
        let scale = 1024.0;
        for step in 0..1000u32 {
            let v = step as f32 / scale;
            assert_eq!(unfixed(fixed(v, scale), scale), v, "round trip failed at {v}");
        }
    }
}
