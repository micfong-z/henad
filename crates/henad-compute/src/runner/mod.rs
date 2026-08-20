//! How a sim loop gets driven, and the one place the two ways of driving one differ.
//!
//! A loop says what work is due and when it next wants calling. A [`Driver`] decides how to wait:
//! a thread of its own on native, the host's frame loop in a browser.

use std::time::Duration;

#[cfg(target_arch = "wasm32")]
mod frame;
#[cfg(not(target_arch = "wasm32"))]
mod thread;

#[cfg(target_arch = "wasm32")]
pub use frame::Driver;
#[cfg(not(target_arch = "wasm32"))]
pub use thread::Driver;

use crate::snapshot::Snapshot;
use std::sync::{Arc, Mutex};

/// Wall clock one pump may reasonably spend.
///
/// The frame driver stops pumping once a frame has spent this much, and a loop that sizes its own
/// batches aims to fill it. Two different numbers would leave a frame running two batches.
pub const PUMP_BUDGET_MS: f64 = 6.0;

/// What a loop wants after one [`SimLoop::pump`].
pub enum Pace {
    /// Nothing until a command arrives.
    Idle,
    /// Again, as soon as the driver can.
    Now,
    /// Again after this long, unless a command arrives first.
    After(Duration),
}

/// A simulation loop, minus how it is driven.
///
/// `pump` does whatever is due now and says when the next work falls due. Everything about
/// blocking, waiting and frame budgets belongs to the driver.
pub trait SimLoop {
    type Command;

    /// True when the loop is finished and the driver should stop.
    fn handle_command(&mut self, cmd: Self::Command) -> bool;

    fn pump(&mut self) -> Pace;

    /// Runs once before the first pump.
    fn start(&mut self) {}
}

/// Where a loop leaves a snapshot for the host to pick up.
///
/// `fresh` is the newest publish waiting to be taken, `spare` a consumed one handed back for its
/// buffers. A `fresh` nobody took is stale by definition, so it becomes the next spare.
#[derive(Default)]
pub struct SnapshotSlot {
    fresh: Option<Snapshot>,
    spare: Option<Snapshot>,
}

/// Shared between a loop and its host. Never sent anywhere on the web, where one thread holds
/// both ends.
pub type SharedSlot = Arc<Mutex<SnapshotSlot>>;

impl SnapshotSlot {
    /// For a loop that publishes its own first snapshot from [`SimLoop::start`], where building
    /// one here would mean reporting stats nothing has read back yet.
    pub fn empty() -> SharedSlot {
        Arc::new(Mutex::new(Self::default()))
    }

    pub fn with_initial(snapshot: Snapshot) -> SharedSlot {
        Arc::new(Mutex::new(Self {
            fresh: Some(snapshot),
            spare: None,
        }))
    }
}

/// `None` when nothing new has been published since the last take.
pub fn take_snapshot(slot: &SharedSlot) -> Option<Snapshot> {
    slot.lock().ok()?.fresh.take()
}

/// Purely an optimisation. Dropping a snapshot instead only means the next publish allocates.
pub fn recycle(slot: &SharedSlot, snapshot: Snapshot) {
    if let Ok(mut slot) = slot.lock() {
        slot.spare = Some(snapshot);
    }
}

/// Takes the spare buffer, if the host handed one back.
pub fn claim_spare(slot: &SharedSlot) -> Option<Snapshot> {
    slot.lock().ok()?.spare.take()
}

/// Hands a freshly built snapshot over.
pub fn publish(slot: &SharedSlot, snapshot: Snapshot) {
    if let Ok(mut slot) = slot.lock() {
        slot.spare = slot.fresh.replace(snapshot);
    }
}
