//! Helpers shared by the tests of the CPU engines and runner.

use std::time::{Duration, Instant};

use henad_core::authoring::primitives::rng::next_bits;
use henad_core::network::Network;

/// Longest time a test waits for a runner to reach the state it expects.
const DEADLINE: Duration = Duration::from_secs(10);

/// Polls `condition` until it holds, and returns whether it held before [`DEADLINE`].
pub(crate) fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Returns a network of `nodes` nodes with an edge for each of `attempts` random pairs drawn from `seed`.
///
/// A pair that would make a self loop or repeat an edge adds nothing.
pub(crate) fn random_network(nodes: u32, directed: bool, seed: u64, attempts: usize) -> Network {
    let mut net = Network::new(nodes as usize, directed);
    let mut rng = seed;
    for _ in 0..attempts {
        let a = next_bits(&mut rng) % nodes;
        let b = next_bits(&mut rng) % nodes;
        if a != b && !net.has_edge(a, b) {
            net.add_edge(a, b, 0);
        }
    }
    net
}

/// Runs `job` inside a new rayon pool of `threads` threads.
pub(crate) fn in_pool_of<R: Send>(threads: usize, job: impl FnOnce() -> R + Send) -> R {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("rayon pool")
        .install(job)
}
