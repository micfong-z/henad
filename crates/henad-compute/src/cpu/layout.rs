//! Spring layout for a network model's node positions.
//!
//! NetLogo's `layout-spring`, with repulsion cut off at a radius and found through a spatial hash.

use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::SpringParams;
use henad_core::authoring::primitives::rng::{mix_seed, next_float};
use henad_core::network::Network;
use henad_core::spatial_hash::SpatialHash;
use rayon::prelude::*;

/// Nodes per chunk of the force gather.
const CHUNK: usize = 512;

/// Largest step per iteration, as a fraction of the world's half perimeter. NetLogo's limit.
const MAX_STEP: f32 = 1.0 / 50.0;

/// Buffers reused across iterations, plus the layout's RNG seed.
pub struct LayoutScratch {
    hash: Option<SpatialHash>,
    disp_x: Vec<f32>,
    disp_y: Vec<f32>,
    seed: u64,
    iteration: u32,
}

impl LayoutScratch {
    pub fn new(seed: u64) -> Self {
        Self {
            hash: None,
            disp_x: Vec::new(),
            disp_y: Vec::new(),
            seed,
            iteration: 0,
        }
    }

    pub fn heap_bytes(&self) -> usize {
        (self.disp_x.capacity() + self.disp_y.capacity()) * size_of::<f32>()
            + self.hash.as_ref().map_or(0, SpatialHash::heap_bytes)
    }
}

/// Spring constants scaled to this world, plus the jitter seed.
#[derive(Clone, Copy)]
struct Forces {
    rest: f32,
    reach: f32,
    push: f32,
    spring: f32,
    seed: u64,
    iteration: u32,
}

/// Runs one iteration of the spring layout.
///
/// All forces are gathered before any node moves, so the thread count cannot change results.
pub fn spring_step(
    pos_x: &mut [f32],
    pos_y: &mut [f32],
    graph: &Network,
    extent: Extent,
    params: SpringParams,
    scratch: &mut LayoutScratch,
) {
    let iteration = scratch.iteration;
    scratch.iteration = scratch.iteration.wrapping_add(1);
    let nodes = graph.node_count();
    if nodes < 2 {
        return;
    }

    // Mean spacing, the unit of every `SpringParams` constant.
    let spacing = (extent.w * extent.h / nodes as f32).sqrt().max(f32::MIN_POSITIVE);
    let forces = Forces {
        rest: params.length * spacing,
        reach: params.cutoff * spacing,
        push: params.repulsion * spacing * spacing * spacing,
        spring: params.spring,
        seed: scratch.seed,
        iteration,
    };
    let limit = (extent.w + extent.h) * MAX_STEP;

    let n = pos_x.len();
    scratch.disp_x.clear();
    scratch.disp_x.resize(n, 0.0);
    scratch.disp_y.clear();
    scratch.disp_y.resize(n, 0.0);

    // Reused while the reach and the world stay the same.
    let mut hash = match scratch.hash.take() {
        Some(hash) if hash.cell_size_is(forces.reach) && hash.world_is(extent.w, extent.h) => hash,
        _ => SpatialHash::new(forces.reach, extent.w, extent.h),
    };
    hash.build_where(pos_x, pos_y, |i| graph.contains_node(i as u32));

    {
        let (px, py) = (&*pos_x, &*pos_y);
        scratch
            .disp_x
            .par_chunks_mut(CHUNK)
            .zip(scratch.disp_y.par_chunks_mut(CHUNK))
            .enumerate()
            .for_each(|(c, (xs, ys))| {
                let base = c * CHUNK;
                for (k, (dx, dy)) in xs.iter_mut().zip(ys.iter_mut()).enumerate() {
                    let i = (base + k) as u32;
                    if !graph.contains_node(i) {
                        continue;
                    }
                    let (fx, fy) = force_on(i, px, py, graph, &hash, forces);
                    *dx = fx.clamp(-limit, limit);
                    *dy = fy.clamp(-limit, limit);
                }
            });
    }

    for i in 0..n {
        if !graph.contains_node(i as u32) {
            continue;
        }
        pos_x[i] = (pos_x[i] + scratch.disp_x[i]).clamp(0.0, extent.w);
        pos_y[i] = (pos_y[i] + scratch.disp_y[i]).clamp(0.0, extent.h);
    }

    scratch.hash = Some(hash);
}

/// Force on node `i` from its edges and from nodes within reach.
#[inline]
fn force_on(i: u32, pos_x: &[f32], pos_y: &[f32], graph: &Network, hash: &SpatialHash, forces: Forces) -> (f32, f32) {
    let Forces {
        rest,
        reach,
        push,
        spring,
        seed,
        iteration,
    } = forces;
    let (xi, yi) = (pos_x[i as usize], pos_y[i as usize]);
    let deg_i = graph.degree(i) as f32;
    let (mut fx, mut fy) = (0.0f32, 0.0f32);

    let mut pull = |j: u32| {
        if j == i {
            return;
        }
        let (ex, ey) = (pos_x[j as usize] - xi, pos_y[j as usize] - yi);
        let d = ex.hypot(ey);
        if d <= 0.0 {
            return;
        }
        // Divided by the mean degree of both ends, as NetLogo does.
        let div = ((deg_i + graph.degree(j) as f32) * 0.5).max(1.0);
        let f = spring * (d - rest) / div;
        fx += f * ex / d;
        fy += f * ey / d;
    };
    for &j in graph.in_neighbors(i) {
        pull(j);
    }
    if graph.directed() {
        for &j in graph.out_neighbors(i) {
            pull(j);
        }
    }

    // The hash wraps at the world's edges and the layout does not, so deltas are recomputed.
    hash.for_each_within(xi, yi, reach, pos_x, pos_y, |j, _wx, _wy, _d2| {
        if j == i {
            return;
        }
        let (ex, ey) = (pos_x[j as usize] - xi, pos_y[j as usize] - yi);
        let d2 = ex * ex + ey * ey;
        if d2 > reach * reach {
            return;
        }
        let div = ((deg_i + graph.degree(j) as f32) * 0.5).max(1.0);
        if d2 <= 0.0 {
            // Coincident, so push along an angle drawn from the node index.
            let angle = jitter_angle(seed, iteration, i);
            fx -= push / div * angle.cos();
            fy -= push / div * angle.sin();
            return;
        }
        let d = d2.sqrt();
        let f = push / d2 / div;
        fx -= f * ex / d;
        fy -= f * ey / d;
    });

    (fx, fy)
}

fn jitter_angle(seed: u64, iteration: u32, i: u32) -> f32 {
    let mut rng = mix_seed(seed ^ (u64::from(iteration) << 40) ^ u64::from(i));
    next_float(&mut rng, std::f32::consts::TAU)
}

#[cfg(test)]
mod tests {
    use super::{LayoutScratch, spring_step};
    use henad_core::authoring::model::field::Extent;
    use henad_core::authoring::model::network_model::SpringParams;
    use henad_core::authoring::primitives::rng::{next_bits, next_float};
    use henad_core::network::Network;

    const EXTENT: Extent = Extent { w: 100.0, h: 100.0 };
    const SEED: u64 = 0x1A70_0001;

    fn relax(net: &Network, pos: &mut (Vec<f32>, Vec<f32>), iterations: u32) {
        let mut scratch = LayoutScratch::new(SEED);
        for _ in 0..iterations {
            spring_step(&mut pos.0, &mut pos.1, net, EXTENT, SpringParams::DEFAULT, &mut scratch);
        }
    }

    fn dist(pos: &(Vec<f32>, Vec<f32>), a: usize, b: usize) -> f32 {
        (pos.0[a] - pos.0[b]).hypot(pos.1[a] - pos.1[b])
    }

    /// Mean spacing of `nodes` nodes in `EXTENT`.
    fn spacing(nodes: usize) -> f32 {
        (EXTENT.w * EXTENT.h / nodes as f32).sqrt()
    }

    #[test]
    fn a_joined_pair_settles_near_the_declared_edge_length() {
        let mut net = Network::new(2, false);
        net.add_edge(0, 1, 0);
        let mut pos = (vec![10.0, 90.0], vec![50.0, 50.0]);
        relax(&net, &mut pos, 400);

        let want = SpringParams::DEFAULT.length * spacing(2);
        let got = dist(&pos, 0, 1);
        assert!(
            (got - want).abs() < 0.35 * want,
            "a joined pair settled at {got}, wanting about {want}"
        );
    }

    #[test]
    fn an_unjoined_pair_pushes_apart() {
        let mut net = Network::new(2, false);
        net.add_edge(0, 1, 0);
        net.remove_edge(0);
        let mut pos = (vec![49.0, 51.0], vec![50.0, 50.0]);
        let before = dist(&pos, 0, 1);
        relax(&net, &mut pos, 40);
        assert!(dist(&pos, 0, 1) > before, "nothing pushed two unjoined nodes apart");
    }

    #[test]
    fn coincident_nodes_separate() {
        let net = Network::new(2, false);
        let mut pos = (vec![50.0, 50.0], vec![50.0, 50.0]);
        relax(&net, &mut pos, 10);
        assert!(dist(&pos, 0, 1) > 0.0, "two nodes on one point stayed there");
        assert!(pos.0.iter().chain(&pos.1).all(|v| v.is_finite()), "a position went bad");
    }

    #[test]
    fn every_node_stays_inside_the_world() {
        let mut net = Network::new(60, false);
        let mut rng = 0xB0_1A_u64;
        for i in 1..60u32 {
            net.add_edge(i - 1, i, 0);
        }
        let mut pos = (Vec::new(), Vec::new());
        for _ in 0..60 {
            pos.0.push(next_float(&mut rng, EXTENT.w));
            pos.1.push(next_float(&mut rng, EXTENT.h));
        }
        relax(&net, &mut pos, 200);

        for (i, (&x, &y)) in pos.0.iter().zip(&pos.1).enumerate() {
            assert!(
                (0.0..=EXTENT.w).contains(&x) && (0.0..=EXTENT.h).contains(&y),
                "node {i} left the world at ({x}, {y})"
            );
        }
    }

    #[test]
    fn a_retired_node_neither_moves_nor_pushes() {
        let mut net = Network::new(3, false);
        net.add_edge(0, 1, 0);
        net.retire(2);
        let mut pos = (vec![40.0, 60.0, f32::NAN], vec![50.0, 50.0, f32::NAN]);
        relax(&net, &mut pos, 30);

        assert!(pos.0[2].is_nan() && pos.1[2].is_nan(), "a retired node was moved");
        assert!(
            pos.0[0].is_finite() && pos.1[0].is_finite() && pos.0[1].is_finite(),
            "a retired node's position leaked into a live one"
        );
    }

    /// Enough nodes to span several chunks.
    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        fn run(threads: usize) -> Vec<u32> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let n = 1500u32;
                let mut net = Network::new(n as usize, false);
                let mut rng = 0x5EED_7777_u64;
                for _ in 0..4000 {
                    let a = next_bits(&mut rng) % n;
                    let b = next_bits(&mut rng) % n;
                    if a != b && !net.has_edge(a, b) {
                        net.add_edge(a, b, 0);
                    }
                }
                let mut pos = (Vec::new(), Vec::new());
                let mut prng = 0xC0DE_u64;
                for _ in 0..n {
                    pos.0.push(next_float(&mut prng, EXTENT.w));
                    pos.1.push(next_float(&mut prng, EXTENT.h));
                }
                relax(&net, &mut pos, 12);
                pos.0.iter().chain(&pos.1).map(|v| v.to_bits()).collect()
            })
        }

        assert_eq!(run(1), run(7), "positions depend on the thread count");
    }
}
