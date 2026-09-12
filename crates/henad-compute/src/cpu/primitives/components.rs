//! Connected components over a network's rows.

use henad_core::network::Network;
use rayon::prelude::*;

/// Nodes per chunk of a propagation pass.
const CHUNK: usize = 4096;

/// Summary of a component labelling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ComponentStats {
    /// Number of components. An isolated node counts as one.
    pub count: usize,
    /// Number of nodes in the largest component.
    pub largest: usize,
}

/// Labels every node with the lowest node index in its component.
///
/// Min-label propagation with pointer jumping. A retired slot keeps its own index and is not
/// counted.
pub fn label_components(graph: &Network, label: &mut Vec<u32>, scratch: &mut Vec<u32>) -> ComponentStats {
    let n = graph.slot_count();
    label.clear();
    label.extend(0..n as u32);
    scratch.clear();
    scratch.resize(n, 0);

    loop {
        let changed = propagate(graph, label, scratch);
        std::mem::swap(label, scratch);
        if !changed {
            break;
        }
        // Two jumps per round shorten the chains propagation leaves behind.
        for _ in 0..2 {
            jump(label, scratch);
            std::mem::swap(label, scratch);
        }
    }

    summarize(graph, label)
}

/// Runs one propagation round. Returns whether any label changed.
fn propagate(graph: &Network, label: &[u32], out: &mut [u32]) -> bool {
    out.par_chunks_mut(CHUNK)
        .enumerate()
        .map(|(c, slice)| {
            let base = c * CHUNK;
            let mut changed = false;
            for (k, cell) in slice.iter_mut().enumerate() {
                let i = (base + k) as u32;
                let mut best = label[i as usize];
                if graph.contains_node(i) {
                    for &j in graph.in_neighbors(i) {
                        best = best.min(label[j as usize]);
                    }
                    if graph.directed() {
                        for &j in graph.out_neighbors(i) {
                            best = best.min(label[j as usize]);
                        }
                    }
                }
                changed |= best != label[i as usize];
                *cell = best;
            }
            changed
        })
        .reduce(|| false, |a, b| a || b)
}

/// Follows every label one step towards its root.
fn jump(label: &[u32], out: &mut [u32]) {
    out.par_chunks_mut(CHUNK).enumerate().for_each(|(c, slice)| {
        let base = c * CHUNK;
        for (k, cell) in slice.iter_mut().enumerate() {
            *cell = label[label[base + k] as usize];
        }
    });
}

fn summarize(graph: &Network, label: &[u32]) -> ComponentStats {
    let mut size = vec![0u32; label.len()];
    for (i, &root) in label.iter().enumerate() {
        if graph.contains_node(i as u32) {
            size[root as usize] += 1;
        }
    }
    ComponentStats {
        count: size.iter().filter(|&&s| s > 0).count(),
        largest: size.iter().copied().max().unwrap_or(0) as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentStats, label_components};
    use henad_core::authoring::primitives::rng::next_bits;
    use henad_core::network::Network;

    /// Reference components from a sequential breadth-first search.
    fn bfs(graph: &Network) -> ComponentStats {
        let n = graph.slot_count();
        let mut seen = vec![false; n];
        let mut stats = ComponentStats::default();
        for start in 0..n as u32 {
            if seen[start as usize] || !graph.contains_node(start) {
                continue;
            }
            let mut queue = vec![start];
            seen[start as usize] = true;
            let mut size = 0;
            while let Some(i) = queue.pop() {
                size += 1;
                let mut visit = |j: u32| {
                    if !seen[j as usize] {
                        seen[j as usize] = true;
                        queue.push(j);
                    }
                };
                for &j in graph.in_neighbors(i) {
                    visit(j);
                }
                if graph.directed() {
                    for &j in graph.out_neighbors(i) {
                        visit(j);
                    }
                }
            }
            stats.count += 1;
            stats.largest = stats.largest.max(size);
        }
        stats
    }

    fn run(graph: &Network) -> ComponentStats {
        let (mut label, mut scratch) = (Vec::new(), Vec::new());
        label_components(graph, &mut label, &mut scratch)
    }

    #[test]
    fn a_graph_with_no_edges_is_all_singletons() {
        let net = Network::new(5, false);
        assert_eq!(run(&net), ComponentStats { count: 5, largest: 1 });
    }

    #[test]
    fn a_chain_is_one_component() {
        let mut net = Network::new(64, false);
        for i in 1..64u32 {
            net.add_edge(i - 1, i, 0);
        }
        assert_eq!(run(&net), ComponentStats { count: 1, largest: 64 });
    }

    #[test]
    fn a_retired_node_counts_for_nothing() {
        let mut net = Network::new(4, false);
        net.add_edge(0, 1, 0);
        net.retire(3);
        assert_eq!(
            run(&net),
            ComponentStats { count: 2, largest: 2 },
            "0-1 joined, 2 alone"
        );
    }

    #[test]
    fn labels_match_a_breadth_first_search() {
        for &directed in &[false, true] {
            for seed in 0..6u64 {
                let mut net = Network::new(120, directed);
                let mut rng = 0x9E37 ^ (seed << 8) ^ u64::from(directed);
                for _ in 0..90 {
                    let a = next_bits(&mut rng) % 120;
                    let b = next_bits(&mut rng) % 120;
                    if a != b && !net.has_edge(a, b) {
                        net.add_edge(a, b, 0);
                    }
                }
                assert_eq!(run(&net), bfs(&net), "directed={directed} seed={seed}");
            }
        }
    }

    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        fn go(threads: usize) -> (ComponentStats, Vec<u32>) {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let n = 9000u32;
                let mut net = Network::new(n as usize, false);
                let mut rng = 0xFACE_u64;
                for _ in 0..7000 {
                    let a = next_bits(&mut rng) % n;
                    let b = next_bits(&mut rng) % n;
                    if a != b && !net.has_edge(a, b) {
                        net.add_edge(a, b, 0);
                    }
                }
                let (mut label, mut scratch) = (Vec::new(), Vec::new());
                let stats = label_components(&net, &mut label, &mut scratch);
                (stats, label)
            })
        }
        assert_eq!(go(1), go(7), "components depend on the thread count");
    }
}
