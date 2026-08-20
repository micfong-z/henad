//! Drives a loop from the host's frame loop, which is all a browser offers.
//!
//! `wasm32-unknown-unknown` cannot spawn a thread even with atomics, so the loop runs inline and
//! the frame has to be handed back. [`PUMP_BUDGET_MS`] is what it may spend before doing so.

use web_time::Instant;

use super::{PUMP_BUDGET_MS, Pace, SimLoop};
use crate::fault::Fault;

pub struct Driver<L: SimLoop> {
    sim: L,
    /// Set by [`Pace::After`], so a capped loop is not pumped early.
    next_pump_at: Instant,
    finished: bool,
}

impl<L: SimLoop> Driver<L> {
    /// `on_fault` is never called. wasm aborts on panic rather than unwinding, so there is nothing
    /// to hand back. The parameter matches the threaded driver and keeps a `cfg` out of the host.
    pub fn spawn(mut sim: L, on_fault: impl FnOnce(Fault) + 'static) -> Self {
        drop(on_fault);
        sim.start();
        Self {
            sim,
            next_pump_at: Instant::now(),
            finished: false,
        }
    }

    pub fn send(&mut self, cmd: L::Command) {
        if self.finished {
            return;
        }
        self.finished = self.sim.handle_command(cmd);
    }

    /// Pumps until the loop has nothing due or the frame budget is spent.
    ///
    /// `dt` is unread. The loop times itself against the same clock the driver uses.
    pub fn update(&mut self, _dt: f64) {
        if self.finished {
            return;
        }
        let start = Instant::now();
        if start < self.next_pump_at {
            return;
        }
        loop {
            match self.sim.pump() {
                Pace::Idle => {
                    self.next_pump_at = start;
                    return;
                }
                Pace::After(wait) => {
                    self.next_pump_at = Instant::now() + wait;
                    return;
                }
                Pace::Now => {
                    if start.elapsed().as_secs_f64() * 1000.0 >= PUMP_BUDGET_MS {
                        self.next_pump_at = start;
                        return;
                    }
                }
            }
        }
    }

    /// No thread to join. The loop is dropped with the driver.
    pub fn shutdown(&mut self, cmd: L::Command) {
        self.send(cmd);
    }
}
