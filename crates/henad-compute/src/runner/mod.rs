//! Drivers of a sim loop, and the one place the native and browser ways of driving one differ.
//!
//! A loop says what work is due and when it next wants calling. A [`Driver`] decides how to wait:
//! its own thread on native, the host's frame loop in a browser.
//!
//! A runner stops within a timeout. A loop handles its stop command only between two pumps, and a model step that
//! never returns would hold the stopping thread for good. A thread that does not stop in time is detached, and
//! [`RunnerShutdown::Detached`] returns its handle.

use std::time::Duration;

#[cfg(any(target_arch = "wasm32", test))]
mod frame;
#[cfg(not(target_arch = "wasm32"))]
mod thread;

#[cfg(target_arch = "wasm32")]
pub use frame::Driver;
#[cfg(not(target_arch = "wasm32"))]
pub use thread::Driver;

/// Whether the target can spawn an OS thread. A browser cannot, even with atomics.
pub const CAN_SPAWN_THREADS: bool = cfg!(not(target_arch = "wasm32"));

use crate::snapshot::Snapshot;
use std::sync::{Arc, Mutex};

/// Wall-clock time in milliseconds that one pump may spend.
///
/// The frame driver stops pumping once a frame has spent this much, and a loop that sizes its own
/// batches aims to fill it. Two different numbers would leave a frame running two batches.
pub const PUMP_BUDGET_MS: f64 = 6.0;

/// Time between two publishes while a loop runs to a tick.
///
/// It is longer than the interval while playing, since each publish copies the whole view.
pub const RUN_TO_PUBLISH_INTERVAL: Duration = Duration::from_millis(100);

/// Most ticks a run to a tick samples its stats at before its target.
///
/// The samples travel in [`Snapshot::run_to_samples`]. Each sample on the device waits for a stats readback, so the
/// count stays small whatever the length of the run.
pub const MAX_RUN_TO_SAMPLES: u64 = 500;

/// Evenly spaced ticks at which a run to a tick samples its stats, at most [`MAX_RUN_TO_SAMPLES`] of them.
///
/// The target itself is never a sample tick. The publish at the target covers it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunToStride {
    /// Number of ticks between two samples.
    stride: u64,
    /// Next tick to sample at.
    next: u64,
    /// Tick the run stops at.
    target: u64,
}

impl RunToStride {
    /// Returns the sample ticks of a run from `tick` to `target`.
    pub(crate) fn new(tick: u64, target: u64) -> Self {
        let stride = target.saturating_sub(tick).div_ceil(MAX_RUN_TO_SAMPLES).max(1);
        Self {
            stride,
            next: tick.saturating_add(stride),
            target,
        }
    }

    /// Returns the number of steps from `tick` to the next sample tick, or `None` once no sample tick is left.
    pub(crate) fn steps_to_next(&self, tick: u64) -> Option<u64> {
        (self.next < self.target).then(|| self.next.saturating_sub(tick))
    }

    /// Returns whether `tick` is due for a sample, and moves on to the next sample tick when it is.
    pub(crate) fn take_due(&mut self, tick: u64) -> bool {
        if tick < self.next || tick >= self.target {
            return false;
        }
        self.next = tick.saturating_add(self.stride);
        true
    }
}

/// Returns the target rate of a capped run, 1 tick per second for a rate that is not finite and positive.
pub(crate) fn capped_tps(target_tps: f64) -> f64 {
    if target_tps.is_finite() && target_tps > 0.0 {
        target_tps
    } else {
        1.0
    }
}

/// Returns the wall-clock time `steps` take at the capped rate of `target_tps`.
pub(crate) fn capped_interval(target_tps: f64, steps: u32) -> Duration {
    Duration::from_secs_f64(f64::from(steps) / capped_tps(target_tps))
}

/// Longest time one publish may spend preparing its view, including a network's layout.
///
/// In a browser, the view is prepared inside the frame pump, so going past the pump's budget stalls the frame.
#[cfg(target_arch = "wasm32")]
pub const MAX_VIEW_BUDGET_MS: f32 = PUMP_BUDGET_MS as f32;

/// Longest time one publish may spend preparing its view, including a network's layout.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_VIEW_BUDGET_MS: f32 = f32::INFINITY;

/// Time a runner's drop waits for its thread to stop before it detaches the thread.
///
/// A stop waits for the pump in progress, one batch of steps. A batch longer than this is rare at interactive
/// scale.
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// Outcome of stopping a runner within a timeout.
#[derive(Debug)]
pub enum RunnerShutdown {
    /// The loop stopped, and its thread, where it has one, was joined.
    Joined,
    /// The thread did not stop within the timeout. It runs on, detached, and stops once its pump returns.
    Detached(DetachedRunner),
}

/// Thread of a runner that did not stop within its timeout.
///
/// Dropping it leaves the thread running. The browser's frame driver has no thread, and never returns one.
#[derive(Debug)]
pub struct DetachedRunner {
    handle: std::thread::JoinHandle<()>,
}

impl DetachedRunner {
    /// Returns whether the thread has finished since it was detached.
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

/// Next pump that a loop requests after one [`SimLoop::pump`].
#[derive(Debug)]
pub enum Pace {
    /// No pump until a command arrives.
    Idle,
    /// Another pump as soon as the driver can.
    Now,
    /// Another pump after this long, unless a command arrives first.
    After(Duration),
}

/// A simulation loop, independent of how it is driven.
///
/// `pump` does whatever is due now and says when the next work falls due. Everything about
/// blocking, waiting and frame budgets belongs to the driver.
pub trait SimLoop {
    /// Command a host sends the loop.
    type Command;

    /// Handles one command, and returns whether the loop is finished and the driver should stop.
    fn handle_command(&mut self, cmd: Self::Command) -> bool;

    /// Does whatever is due now, and returns when the loop next wants a pump.
    fn pump(&mut self) -> Pace;

    /// Runs once before the first pump.
    fn start(&mut self) {}
}

/// Slot where a loop leaves a snapshot for the host to pick up.
///
/// `fresh` is the newest publish waiting to be taken, `spare` a consumed one returned for its
/// buffers. A `fresh` nobody took is stale by definition, so it becomes the next spare.
#[derive(Debug, Default)]
pub struct SnapshotSlot {
    fresh: Option<Snapshot>,
    spare: Option<Snapshot>,
}

/// Snapshot slot shared between a loop and its host. On the web one thread holds both ends, and the slot is never
/// sent anywhere.
pub type SharedSlot = Arc<Mutex<SnapshotSlot>>;

impl SnapshotSlot {
    /// Returns an empty slot, for a loop that publishes its own first snapshot from [`SimLoop::start`].
    ///
    /// Building one here would mean reporting stats nothing has read back yet.
    #[cfg_attr(
        all(target_arch = "wasm32", target_feature = "atomics"),
        expect(
            clippy::arc_with_non_send_sync,
            reason = "a snapshot holds wgpu handles, which atomics leave unsendable"
        )
    )]
    pub fn empty() -> SharedSlot {
        Arc::new(Mutex::new(Self::default()))
    }

    /// Returns a slot holding `snapshot` as its first publish.
    #[cfg_attr(
        all(target_arch = "wasm32", target_feature = "atomics"),
        expect(
            clippy::arc_with_non_send_sync,
            reason = "a snapshot holds wgpu handles, which atomics leave unsendable"
        )
    )]
    pub fn with_initial(snapshot: Snapshot) -> SharedSlot {
        Arc::new(Mutex::new(Self {
            fresh: Some(snapshot),
            spare: None,
        }))
    }
}

/// Takes the latest publish, or `None` when nothing new has been published since the last take.
pub fn take_snapshot(slot: &SharedSlot) -> Option<Snapshot> {
    slot.lock().ok()?.fresh.take()
}

/// Stores a taken snapshot as the spare for the next publish to refill.
///
/// This is only an optimisation. Dropping a snapshot instead only means the next publish allocates.
pub fn recycle(slot: &SharedSlot, snapshot: Snapshot) {
    if let Ok(mut slot) = slot.lock() {
        slot.spare = Some(snapshot);
    }
}

/// Takes the spare buffer, if the host returned one.
pub fn claim_spare(slot: &SharedSlot) -> Option<Snapshot> {
    slot.lock().ok()?.spare.take()
}

/// Passes a freshly built snapshot to the slot.
///
/// The samples of a publish the host has not taken move to the front of `snapshot`'s samples.
pub fn publish(slot: &SharedSlot, mut snapshot: Snapshot) {
    if let Ok(mut slot) = slot.lock() {
        if let Some(untaken) = slot.fresh.as_mut()
            && !untaken.run_to_samples.is_empty()
        {
            untaken.run_to_samples.append(&mut snapshot.run_to_samples);
            std::mem::swap(&mut untaken.run_to_samples, &mut snapshot.run_to_samples);
        }
        slot.spare = slot.fresh.replace(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::{capped_interval, capped_tps};
    use std::time::Duration;

    /// Batching must not multiply the tick rate by the batch size.
    #[test]
    fn batching_does_not_change_effective_tick_rate() {
        for &tps in &[1.0, 30.0, 250.0, 1000.0] {
            for &batch in &[1, 2, 10, 137, 1000] {
                let interval = capped_interval(tps, batch).as_secs_f64();
                let effective = f64::from(batch) / interval;
                // A `Duration` holds whole nanoseconds, and the shortest interval here is a millisecond.
                assert!(
                    (effective - tps).abs() < tps * 1e-6,
                    "tps {tps}, batch {batch}: effective {effective}"
                );
            }
        }
    }

    #[test]
    fn interval_is_batch_size_over_tps() {
        assert_eq!(capped_interval(30.0, 10), Duration::from_secs_f64(1.0 / 3.0));
        assert_eq!(capped_interval(60.0, 1), Duration::from_secs_f64(1.0 / 60.0));
    }

    /// Guards against a `Duration::from_secs_f64` panic on a degenerate target rate.
    #[test]
    fn non_positive_tps_yields_a_finite_interval() {
        for &tps in &[0.0, -5.0, f64::NAN, f64::INFINITY] {
            let capped = capped_tps(tps);
            assert!(capped.is_finite() && capped > 0.0, "tps {tps} gave {capped}");
            assert!(capped_interval(tps, 4) > Duration::ZERO);
        }
    }
}
