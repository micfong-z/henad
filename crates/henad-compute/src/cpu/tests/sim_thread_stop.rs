//! Tests of a runner whose step blocks until the test releases it, as a model step that never returns would.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::StatEntry;

use crate::cpu::sim_thread::SimThread;
use crate::cpu::tests::support::wait_until;
use crate::fault::FaultSink;
use crate::runner::{DEFAULT_STOP_TIMEOUT, RunnerShutdown};

/// Time a stop may take past its timeout, for the scheduler.
const SLACK: Duration = Duration::from_secs(1);

/// State whose step blocks until its gate receives, or the sender is dropped.
struct Gated {
    gate: Receiver<()>,
    tick: u64,
    /// Set when a step starts.
    entered: Arc<AtomicBool>,
    /// Set when the state is dropped. The loop drops it on its thread as it ends.
    dropped: Arc<AtomicBool>,
}

/// Flags a test reads from a [`Gated`] state on its thread.
struct GatedFlags {
    entered: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

/// Returns a gated state, the sender that releases its step, and its flags.
fn gated() -> (Gated, Sender<()>, GatedFlags) {
    let (release, gate) = mpsc::channel();
    let flags = GatedFlags {
        entered: Arc::new(AtomicBool::new(false)),
        dropped: Arc::new(AtomicBool::new(false)),
    };
    let state = Gated {
        gate,
        tick: 0,
        entered: Arc::clone(&flags.entered),
        dropped: Arc::clone(&flags.dropped),
    };
    (state, release, flags)
}

impl Drop for Gated {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl SimState for Gated {
    fn step(&mut self) {
        self.entered.store(true, Ordering::SeqCst);
        // A dropped sender releases the step too.
        self.gate.recv().unwrap_or_default();
        self.tick += 1;
    }
    fn tick(&self) -> u64 {
        self.tick
    }
    fn stats(&self) -> Vec<StatEntry> {
        Vec::new()
    }
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }
    fn population(&self) -> u64 {
        0
    }
    fn heap_bytes(&self) -> usize {
        0
    }
}

/// Returns a runner over a gated state, stepping its first step, with the step's sender and flags.
fn blocked_runner() -> (SimThread, Sender<()>, GatedFlags) {
    let (state, release, flags) = gated();
    let mut thread = SimThread::new(Box::new(state), 60.0, None, FaultSink::new());
    thread.step_once();
    assert!(
        wait_until(|| flags.entered.load(Ordering::SeqCst)),
        "the step never started"
    );
    (thread, release, flags)
}

#[test]
fn a_runner_that_never_returns_is_detached_within_the_timeout() {
    let (thread, release, flags) = blocked_runner();
    let timeout = Duration::from_millis(200);

    let start = Instant::now();
    let RunnerShutdown::Detached(detached) = thread.stop(timeout) else {
        panic!("a runner blocked in its step was joined");
    };
    let elapsed = start.elapsed();
    assert!(elapsed >= timeout, "detached after {elapsed:?}, before the timeout");
    assert!(elapsed < timeout + SLACK, "the stop waited {elapsed:?}");
    assert!(!detached.is_finished(), "the blocked thread finished");

    // Released, the step returns and the loop reads the stop command it was sent.
    release.send(()).expect("the step holds the gate");
    assert!(
        wait_until(|| detached.is_finished()),
        "the released thread never finished"
    );
    assert!(flags.dropped.load(Ordering::SeqCst), "the state outlived its loop");
}

#[test]
fn dropping_a_runaway_runner_returns_within_the_timeout() {
    let (thread, release, flags) = blocked_runner();

    let start = Instant::now();
    drop(thread);
    let elapsed = start.elapsed();
    assert!(
        elapsed >= DEFAULT_STOP_TIMEOUT,
        "the drop returned after {elapsed:?}, before the timeout"
    );
    assert!(elapsed < DEFAULT_STOP_TIMEOUT + SLACK, "the drop waited {elapsed:?}");

    drop(release);
    assert!(
        wait_until(|| flags.dropped.load(Ordering::SeqCst)),
        "the released thread never ended"
    );
}

/// A runner whose step returns stops at once, without the timeout.
#[test]
fn a_runner_that_stops_is_joined() {
    let (state, release, flags) = gated();
    drop(release);
    let thread = SimThread::new(Box::new(state), 60.0, None, FaultSink::new());

    let start = Instant::now();
    assert!(matches!(thread.stop(DEFAULT_STOP_TIMEOUT), RunnerShutdown::Joined));
    assert!(start.elapsed() < SLACK, "the stop waited {:?}", start.elapsed());
    assert!(flags.dropped.load(Ordering::SeqCst), "the joined loop kept its state");
}

/// State that records the width of the pool its step and its view preparation run on.
struct PoolWidth {
    tick: u64,
    step_width: Arc<AtomicUsize>,
    view_width: Arc<AtomicUsize>,
}

impl SimState for PoolWidth {
    fn step(&mut self) {
        assert!(rayon::current_thread_index().is_some(), "the step ran outside a pool");
        self.step_width.store(rayon::current_num_threads(), Ordering::SeqCst);
        self.tick += 1;
    }
    fn tick(&self) -> u64 {
        self.tick
    }
    fn prepare_view(&mut self) {
        self.view_width.store(rayon::current_num_threads(), Ordering::SeqCst);
    }
    fn stats(&self) -> Vec<StatEntry> {
        Vec::new()
    }
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }
    fn population(&self) -> u64 {
        0
    }
    fn heap_bytes(&self) -> usize {
        0
    }
}

/// The first snapshot and every step of a runner started in a pool run on that pool, and the global pool runs
/// none of it.
#[test]
fn a_runner_in_a_pool_steps_on_its_workers() {
    let width = rayon::current_num_threads() + 1;
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(width)
            .build()
            .expect("a pool of a few threads"),
    );
    let step_width = Arc::new(AtomicUsize::new(0));
    let view_width = Arc::new(AtomicUsize::new(0));
    let state = PoolWidth {
        tick: 0,
        step_width: Arc::clone(&step_width),
        view_width: Arc::clone(&view_width),
    };

    let mut thread = SimThread::new_in(pool, Box::new(state), 60.0, None, FaultSink::new());
    assert_eq!(view_width.load(Ordering::SeqCst), width, "the first snapshot");
    thread.step_once();
    assert!(
        wait_until(|| step_width.load(Ordering::SeqCst) != 0),
        "the step never ran"
    );
    assert_eq!(step_width.load(Ordering::SeqCst), width);
    assert!(matches!(thread.stop(DEFAULT_STOP_TIMEOUT), RunnerShutdown::Joined));
}
