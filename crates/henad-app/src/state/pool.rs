//! Thread pool the app's CPU work runs in: model builds and runners, the viewport's uploads and the Sweep tab's column
//! builds.
//!
//! The pool starts as rayon's global pool. A run or a build that has not stopped [`DEFAULT_STOP_TIMEOUT`] after the app
//! stopped it can hold workers of its pool for good, and retires that pool. The app's CPU work then moves to a fresh
//! pool at the global width. Each pool has a generation, and a detached thread records the generation of the pool it
//! runs in.
//!
//! [`DEFAULT_STOP_TIMEOUT`]: henad_compute::runner::DEFAULT_STOP_TIMEOUT

use std::sync::{Arc, Mutex, PoisonError};

/// Shared handle on the pool the app's CPU work runs in. Clones see the same pool and its replacements.
#[derive(Clone, Default)]
pub struct ModelPool(Arc<Mutex<PoolSlot>>);

/// Current pool and its generation.
#[derive(Default)]
struct PoolSlot {
    /// Number of pools retired so far.
    generation: u64,
    /// The pool, `None` for rayon's global pool.
    pool: Option<Arc<rayon::ThreadPool>>,
}

impl ModelPool {
    /// Returns the generation of the current pool and the pool, `None` for rayon's global pool.
    pub fn current(&self) -> (u64, Option<Arc<rayon::ThreadPool>>) {
        let slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        (slot.generation, slot.pool.clone())
    }

    /// Runs `work` on a worker of the current pool, or on the calling thread for rayon's global pool, and returns
    /// its result.
    pub fn install<R: Send>(&self, work: impl FnOnce() -> R + Send) -> R {
        match self.current().1 {
            Some(pool) => pool.install(work),
            None => work(),
        }
    }

    /// Puts a fresh pool at the global width in place of the pool of `generation`. Does nothing when that pool has
    /// been retired already.
    ///
    /// The threads that hold the old pool keep it alive, and a pool nobody holds ends its workers once they are idle.
    /// A pool that cannot be built leaves the old one in place under the next generation.
    pub fn retire(&self, generation: u64) {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.generation != generation {
            return;
        }
        slot.generation += 1;
        let width = rayon::current_num_threads();
        match rayon::ThreadPoolBuilder::new()
            .num_threads(width)
            .thread_name(|index| format!("henad-pool-{index}"))
            .build()
        {
            Ok(pool) => {
                log::info!("CPU models now run in a fresh pool of {width} threads");
                slot.pool = Some(Arc::new(pool));
            }
            Err(error) => log::error!("No fresh thread pool, and CPU models stay in the old one: {error}"),
        }
    }
}
