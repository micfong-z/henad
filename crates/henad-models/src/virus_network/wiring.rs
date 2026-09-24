//! Generators for the initial edges, and the rewire that moves an edge.

use std::f64::consts::PI;

use henad_compute::cpu::primitives::chunked::reduce_chunks;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::primitives::rng::{next_bits, next_index};
use henad_core::network::Network;
use henad_core::spatial_hash::SpatialHash;

use crate::virus_network::{EDGE_OPEN, edge_color};

/// Number of nodes per chunk of the geometric pair search.
const PAIR_CHUNK: usize = 4096;

/// Number of draws a rewire makes before giving up.
///
/// A nearly complete graph has almost nowhere to move an edge to.
pub(crate) const REWIRE_TRIES: u32 = 64;

/// Draws two distinct nodes uniformly at random.
fn random_pair(n: u32, rng: &mut u64) -> (u32, u32) {
    let a = next_index(rng, n);
    let b = next_index(rng, n - 1);
    (a, if b >= a { b + 1 } else { b })
}

/// Returns whether an edge joins `a` and `b` in either direction.
///
/// Checking both directions keeps the graph simple when a directed graph is read as undirected.
fn joined(graph: &Network, a: u32, b: u32) -> bool {
    graph.has_edge(a, b) || (graph.directed() && graph.has_edge(b, a))
}

/// Adds the edges of a uniform random graph with `ceil(degree * n / 2)` edges.
///
/// This matches the loop in the NetLogo fixture procedure, which runs `create-link-with one-of other turtles`
/// until the count is reached. A pair drawn twice is skipped, so every simple graph of that size is equally likely.
pub(super) fn random(graph: &mut Network, degree: u32, rng: &mut u64) {
    let n = graph.slot_count() as u64;
    if n < 2 {
        return;
    }
    let wanted = (u64::from(degree) * n).div_ceil(2).min(n * (n - 1) / 2) as usize;
    while graph.edge_count() < wanted {
        let (a, b) = random_pair(n as u32, rng);
        if !joined(graph, a, b) {
            graph.add_edge(a, b, EDGE_OPEN);
        }
    }
}

/// Joins every pair of nodes closer than a radius chosen so that the mean degree comes out at `degree`.
///
/// Positions are uniform over `area`, which sits inside `world`. Each edge points in a random direction,
/// so switching to directed does not leave every edge pointing up the index order.
pub(super) fn geometric(
    graph: &mut Network,
    pos_x: &[f32],
    pos_y: &[f32],
    area: Extent,
    world: Extent,
    degree: u32,
    rng: &mut u64,
) {
    let n = pos_x.len();
    if n < 2 {
        return;
    }
    let share = f64::from(degree) / (n - 1) as f64;
    let reach = reach_for(share, f64::from(area.w), f64::from(area.h)) as f32;
    let reach_sq = reach * reach;

    let mut hash = SpatialHash::new(reach, world.w, world.h);
    hash.build(pos_x, pos_y);

    // The hash measures distances across the seam but the world has none, so each candidate is measured again
    // without wrapping.
    let pairs = reduce_chunks(
        n,
        PAIR_CHUNK,
        |range| {
            let mut found = Vec::new();
            for i in range {
                let (x, y) = (pos_x[i], pos_y[i]);
                hash.for_each_within(x, y, reach, pos_x, pos_y, |j, _, _, _| {
                    let (dx, dy) = (pos_x[j as usize] - x, pos_y[j as usize] - y);
                    if j as usize > i && dx * dx + dy * dy <= reach_sq {
                        found.push((i as u32, j));
                    }
                });
            }
            found
        },
        |mut all, chunk| {
            all.extend(chunk);
            all
        },
        Vec::new(),
    );

    for (i, j) in pairs {
        let (a, b) = if next_bits(rng) & 1 == 0 { (i, j) } else { (j, i) };
        graph.add_edge(a, b, EDGE_OPEN);
    }
}

/// Returns the distance within which a pair of uniform points in a `w` by `h` rectangle falls with probability `share`.
///
/// This inverts the distance distribution `(π w h r² - 4/3 (w + h) r³ + r⁴ / 2) / (w h)²`,
/// which holds up to the shorter side.
/// A node near the edge has part of its disc outside the world,
/// so sizing the disc by `π r²` alone would leave the mean degree short by that part.
fn reach_for(share: f64, w: f64, h: f64) -> f64 {
    let within = |r: f64| (PI * w * h * r * r - 4.0 / 3.0 * (w + h) * r.powi(3) + 0.5 * r.powi(4)) / (w * w * h * h);
    let (mut lo, mut hi) = (0.0, w.min(h));
    if within(hi) <= share {
        return hi;
    }
    for _ in 0..64 {
        let mid = 0.5 * (lo + hi);
        if within(mid) < share {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

/// Moves one random edge to a random pair of nodes that are not already joined.
///
/// The edge count stays the same and the graph stays simple. This follows NetLogo's `rewire-a-link` from
/// Diffusion on a Directed Network, except that every unjoined pair is a candidate, rather than a fixed set of
/// inactive links.
pub(super) fn rewire(graph: &mut Network, state: &[u8], rng: &mut u64) {
    let (n, m) = (graph.slot_count() as u32, graph.edge_count() as u32);
    if n < 2 || m == 0 {
        return;
    }
    for _ in 0..REWIRE_TRIES {
        let (a, b) = random_pair(n, rng);
        if !joined(graph, a, b) {
            graph.remove_edge(next_index(rng, m));
            graph.add_edge(a, b, edge_color(state[a as usize], state[b as usize]));
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reach_for;

    /// Samples the rectangle that the formula describes, so that a sign error in the formula would show.
    #[test]
    fn the_reach_matches_a_sampled_rectangle() {
        use henad_core::authoring::primitives::rng::next_float;

        let (w, h) = (300.0f32, 200.0f32);
        let share = 0.05;
        let r = reach_for(share, f64::from(w), f64::from(h)) as f32;

        let mut rng = 0x7EAC_4000_0000_0001;
        let trials = 400_000;
        let mut within = 0u32;
        for _ in 0..trials {
            let (ax, ay) = (next_float(&mut rng, w), next_float(&mut rng, h));
            let (bx, by) = (next_float(&mut rng, w), next_float(&mut rng, h));
            if (ax - bx).hypot(ay - by) <= r {
                within += 1;
            }
        }
        let got = f64::from(within) / f64::from(trials);
        assert!(
            (got - share).abs() < 0.002,
            "sampled {got} of pairs within reach, wanting {share}"
        );
    }

    #[test]
    fn a_share_past_the_shorter_side_stops_there() {
        assert_eq!(reach_for(1.0, 10.0, 20.0), 10.0);
    }
}
