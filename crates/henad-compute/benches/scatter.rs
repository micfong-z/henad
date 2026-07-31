//! Which scatter strategy to back `henad_compute::scatter` with. Atomics, counting sort, and
//! per-worker shadow grids, all producing `out[c] = combine(base[c], values landing in c)`.
//!
//! Density is swept by shrinking the grid at a fixed agent count, so every configuration does the
//! same number of deposits and only the collision rate moves. Thread count comes from explicit
//! rayon pools, since the global one is built once per process.

use std::hint::black_box;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use henad_core::helpers::xorshift64;
use rayon::prelude::*;

/// There is no atomic float add, and a CAS loop over f32 would depend on arrival order.
const SUM_SCALE: f32 = 1024.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Distribution {
    Uniform,
    /// 90% of agents in a contiguous 2% of the grid.
    ///
    /// Contiguous in flat index order, so harsher than a real trail. It maximises cache line
    /// sharing between colliding agents, which is what would sink the atomic route.
    Clustered,
}

impl Distribution {
    fn label(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Clustered => "clustered",
        }
    }
}

/// Generated once and reused across every strategy and sample.
struct Workload {
    cells: Vec<u32>,
    values: Vec<f32>,
    base: Vec<f32>,
    n_cells: usize,
}

impl Workload {
    fn new(n_agents: usize, n_cells: usize, dist: Distribution, mut seed: u64) -> Self {
        let mut next = move || {
            seed = xorshift64(seed);
            seed
        };

        let band = (n_cells / 50).max(1);
        let mut cells = Vec::with_capacity(n_agents);
        let mut values = Vec::with_capacity(n_agents);
        for _ in 0..n_agents {
            let r = next();
            // Own draw, so band membership stays independent of the cell draw.
            let in_band = dist == Distribution::Clustered && next() % 10 != 0;
            let cell = if in_band {
                r as usize % band
            } else {
                r as usize % n_cells
            };
            cells.push(cell as u32);
            // Non-negative for `AtomicMax`, bounded so the fixed point sum cannot overflow.
            values.push((next() >> 40) as f32 / 16_777_216.0);
        }

        let base = (0..n_cells).map(|_| (next() >> 40) as f32 / 16_777_216.0).collect();

        Self {
            cells,
            values,
            base,
            n_cells,
        }
    }
}

/// `fetch_max` on f32 bit patterns.
///
/// Works because non-negative f32 bit patterns order the same as the floats. A negative value
/// would invert that via the sign bit, and a NaN would beat `f32::MAX`.
struct AtomicMax {
    acc: Vec<AtomicU32>,
}

impl AtomicMax {
    fn new(n_cells: usize) -> Self {
        Self {
            acc: (0..n_cells).map(|_| AtomicU32::new(0)).collect(),
        }
    }

    fn run(&self, cells: &[u32], values: &[f32], base: &[f32], out: &mut [f32]) {
        let acc = &self.acc;
        cells.par_iter().zip(values.par_iter()).for_each(|(&c, &v)| {
            acc[c as usize].fetch_max(v.to_bits(), Ordering::Relaxed);
        });
        // The rayon join above is the happens-before edge, so `Relaxed` is enough on the RMW.
        out.par_iter_mut().enumerate().for_each(|(c, o)| {
            *o = base[c].max(f32::from_bits(acc[c].swap(0, Ordering::Relaxed)));
        });
    }
}

/// u64 rather than u32 because atomic add wraps rather than saturating.
struct AtomicSum {
    acc: Vec<AtomicU64>,
}

impl AtomicSum {
    fn new(n_cells: usize) -> Self {
        Self {
            acc: (0..n_cells).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    fn run(&self, cells: &[u32], values: &[f32], base: &[f32], out: &mut [f32]) {
        let acc = &self.acc;
        cells.par_iter().zip(values.par_iter()).for_each(|(&c, &v)| {
            acc[c as usize].fetch_add((v * SUM_SCALE) as u64, Ordering::Relaxed);
        });
        out.par_iter_mut().enumerate().for_each(|(c, o)| {
            *o = base[c] + acc[c].swap(0, Ordering::Relaxed) as f32 / SUM_SCALE;
        });
    }
}

/// Counting sort by cell, turning the scatter into a gather.
///
/// Two departures from `SpatialHash::build`. The write cursor is retained rather than cloned per
/// call, and the sort permutes values rather than agent indices so the reduce reads contiguously.
struct CellSort {
    cell_start: Vec<u32>,
    write_pos: Vec<u32>,
    sorted_values: Vec<f32>,
}

impl CellSort {
    fn new(n_cells: usize, n_agents: usize) -> Self {
        Self {
            cell_start: vec![0; n_cells + 1],
            write_pos: vec![0; n_cells + 1],
            sorted_values: vec![0.0; n_agents],
        }
    }

    fn build(&mut self, cells: &[u32], values: &[f32]) {
        let n_cells = self.cell_start.len() - 1;
        self.cell_start.fill(0);
        for &c in cells {
            self.cell_start[c as usize + 1] += 1;
        }
        for i in 1..=n_cells {
            self.cell_start[i] += self.cell_start[i - 1];
        }
        self.write_pos.copy_from_slice(&self.cell_start);
        for (i, &c) in cells.iter().enumerate() {
            let p = self.write_pos[c as usize];
            self.sorted_values[p as usize] = values[i];
            self.write_pos[c as usize] = p + 1;
        }
    }

    fn reduce_max(&self, base: &[f32], out: &mut [f32]) {
        let (starts, sorted) = (&self.cell_start, &self.sorted_values);
        out.par_iter_mut().enumerate().for_each(|(c, o)| {
            let run = &sorted[starts[c] as usize..starts[c + 1] as usize];
            *o = run.iter().fold(base[c], |m, &v| m.max(v));
        });
    }

    fn reduce_sum(&self, base: &[f32], out: &mut [f32]) {
        let (starts, sorted) = (&self.cell_start, &self.sorted_values);
        out.par_iter_mut().enumerate().for_each(|(c, o)| {
            let run = &sorted[starts[c] as usize..starts[c + 1] as usize];
            // Fixed point, so this is bit-identical to `AtomicSum` rather than merely close.
            let total: u64 = run.iter().map(|&v| (v * SUM_SCALE) as u64).sum();
            *o = base[c] + total as f32 / SUM_SCALE;
        });
    }
}

/// One private grid per worker, reduced afterwards. No atomics and no sort, at `threads x W x H`
/// memory.
struct ShadowMax {
    shadows: Vec<Vec<f32>>,
}

impl ShadowMax {
    fn new(n_cells: usize) -> Self {
        Self {
            shadows: (0..rayon::current_num_threads()).map(|_| vec![0.0; n_cells]).collect(),
        }
    }

    fn run(&mut self, cells: &[u32], values: &[f32], base: &[f32], out: &mut [f32]) {
        let chunk = cells.len().div_ceil(self.shadows.len().max(1)).max(1);
        // Clearing is a per-call cost, not setup. The reduce cannot consume the shadows the way
        // the atomic merge consumes its accumulator with `swap`.
        self.shadows
            .par_iter_mut()
            .zip(cells.par_chunks(chunk))
            .zip(values.par_chunks(chunk))
            .for_each(|((shadow, cs), vs)| {
                shadow.fill(0.0);
                for (&c, &v) in cs.iter().zip(vs) {
                    let slot = &mut shadow[c as usize];
                    if v > *slot {
                        *slot = v;
                    }
                }
            });

        // A chunk per shadow, so any shadow past the chunk count was never cleared this call.
        let live = cells.len().div_ceil(chunk).min(self.shadows.len());
        let shadows = &self.shadows[..live];
        out.par_iter_mut().enumerate().for_each(|(c, o)| {
            *o = shadows.iter().fold(base[c], |m, s| m.max(s[c]));
        });
    }
}

/// Without this the timings could be comparing different operations.
fn assert_strategies_agree() {
    for dist in [Distribution::Uniform, Distribution::Clustered] {
        let w = Workload::new(50_000, 512, dist, 0x51CA_7737_0BEE_F001);
        let mut sort = CellSort::new(w.n_cells, w.cells.len());
        sort.build(&w.cells, &w.values);

        let mut expected = vec![0.0f32; w.n_cells];
        AtomicMax::new(w.n_cells).run(&w.cells, &w.values, &w.base, &mut expected);

        let mut got = vec![0.0f32; w.n_cells];
        sort.reduce_max(&w.base, &mut got);
        assert_bits_eq(&expected, &got, "SortMax", dist);

        got.fill(0.0);
        ShadowMax::new(w.n_cells).run(&w.cells, &w.values, &w.base, &mut got);
        assert_bits_eq(&expected, &got, "ShadowMax", dist);

        AtomicSum::new(w.n_cells).run(&w.cells, &w.values, &w.base, &mut expected);
        got.fill(0.0);
        sort.reduce_sum(&w.base, &mut got);
        assert_bits_eq(&expected, &got, "SortSum", dist);
    }
}

fn assert_bits_eq(expected: &[f32], got: &[f32], who: &str, dist: Distribution) {
    let mismatch = expected.iter().zip(got).position(|(a, b)| a.to_bits() != b.to_bits());
    assert!(
        mismatch.is_none(),
        "{who} disagrees with the atomic reference on the {} workload at cell {mismatch:?}",
        dist.label()
    );
}

const AGENT_COUNTS: [usize; 2] = [1_000_000, 10_000_000];
/// Mean agents per cell. 100 brackets the target regime rather than sitting at its edge.
const DENSITIES: [usize; 3] = [1, 10, 100];

fn bench_max(c: &mut Criterion) {
    assert_strategies_agree();

    let mut group = c.benchmark_group("scatter_max");
    group.sample_size(20);

    for n_agents in AGENT_COUNTS {
        for density in DENSITIES {
            for dist in [Distribution::Uniform, Distribution::Clustered] {
                let n_cells = n_agents / density;
                let w = Workload::new(n_agents, n_cells, dist, 0x51CA_7737_0BEE_F002);
                let mut out = vec![0.0f32; n_cells];
                let id = format!("{}M/d{density}/{}", n_agents / 1_000_000, dist.label());
                group.throughput(Throughput::Elements(n_agents as u64));

                let atomic = AtomicMax::new(n_cells);
                group.bench_function(BenchmarkId::new("atomic", &id), |b| {
                    b.iter(|| {
                        atomic.run(&w.cells, &w.values, &w.base, &mut out);
                        black_box(&out);
                    });
                });
                drop(atomic);

                let mut sort = CellSort::new(n_cells, n_agents);
                group.bench_function(BenchmarkId::new("sort", &id), |b| {
                    b.iter(|| {
                        sort.build(&w.cells, &w.values);
                        sort.reduce_max(&w.base, &mut out);
                        black_box(&out);
                    });
                });
                drop(sort);

                // Skipped where the shadows alone would want more memory than the machine has.
                if n_cells * 4 * rayon::current_num_threads() < 4 << 30 {
                    let mut shadow = ShadowMax::new(n_cells);
                    group.bench_function(BenchmarkId::new("shadow", &id), |b| {
                        b.iter(|| {
                            shadow.run(&w.cells, &w.values, &w.base, &mut out);
                            black_box(&out);
                        });
                    });
                }
            }
        }
    }

    group.finish();
}

/// Additive scatter, at the density extremes only.
///
/// `fetch_add` contends like `fetch_max`, so the rest would restate `bench_max`. The point is
/// whether the wider u64 accumulator, halving cells per cache line, flips the ordering.
fn bench_sum(c: &mut Criterion) {
    let mut group = c.benchmark_group("scatter_sum");
    group.sample_size(20);

    let n_agents = 10_000_000;
    for density in [1, 100] {
        for dist in [Distribution::Uniform, Distribution::Clustered] {
            let n_cells = n_agents / density;
            let w = Workload::new(n_agents, n_cells, dist, 0x51CA_7737_0BEE_F003);
            let mut out = vec![0.0f32; n_cells];
            let id = format!("10M/d{density}/{}", dist.label());
            group.throughput(Throughput::Elements(n_agents as u64));

            let atomic = AtomicSum::new(n_cells);
            group.bench_function(BenchmarkId::new("atomic", &id), |b| {
                b.iter(|| {
                    atomic.run(&w.cells, &w.values, &w.base, &mut out);
                    black_box(&out);
                });
            });
            drop(atomic);

            let mut sort = CellSort::new(n_cells, n_agents);
            group.bench_function(BenchmarkId::new("sort", &id), |b| {
                b.iter(|| {
                    sort.build(&w.cells, &w.values);
                    sort.reduce_sum(&w.base, &mut out);
                    black_box(&out);
                });
            });
        }
    }

    group.finish();
}

/// Thread scaling at the density extremes, on the clustered workload.
///
/// `t1` is the control. With one worker there is no contention to avoid and shadow holds a single
/// grid, so a lead that survives it means less work rather than less waiting.
fn bench_threads(c: &mut Criterion) {
    let all = rayon::current_num_threads();
    let mut group = c.benchmark_group("scatter_threads");
    group.sample_size(20);

    let n_agents = 10_000_000;
    for density in [1, 100] {
        let n_cells = n_agents / density;
        let w = Workload::new(n_agents, n_cells, Distribution::Clustered, 0x51CA_7737_0BEE_F004);
        let mut out = vec![0.0f32; n_cells];
        group.throughput(Throughput::Elements(n_agents as u64));

        for threads in [1, 4, all] {
            let Ok(pool) = rayon::ThreadPoolBuilder::new().num_threads(threads).build() else {
                continue;
            };
            let id = format!("d{density}/t{threads}");

            let atomic = AtomicMax::new(n_cells);
            group.bench_function(BenchmarkId::new("atomic", &id), |b| {
                b.iter(|| {
                    pool.install(|| atomic.run(&w.cells, &w.values, &w.base, &mut out));
                    black_box(&out);
                });
            });
            drop(atomic);

            let mut sort = CellSort::new(n_cells, n_agents);
            group.bench_function(BenchmarkId::new("sort", &id), |b| {
                b.iter(|| {
                    pool.install(|| {
                        sort.build(&w.cells, &w.values);
                        sort.reduce_max(&w.base, &mut out);
                    });
                    black_box(&out);
                });
            });
            drop(sort);

            if n_cells * 4 * threads < 4 << 30 {
                // Built inside the pool so it sizes its shadow count to *this* pool, not the global one.
                let mut shadow = pool.install(|| ShadowMax::new(n_cells));
                group.bench_function(BenchmarkId::new("shadow", &id), |b| {
                    b.iter(|| {
                        pool.install(|| shadow.run(&w.cells, &w.values, &w.base, &mut out));
                        black_box(&out);
                    });
                });
            }
        }
    }

    group.finish();
}

criterion_group!(benches, bench_max, bench_sum, bench_threads);
criterion_main!(benches);
