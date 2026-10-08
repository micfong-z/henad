//! The native driver, which runs a sim loop on its own OS thread so stepping never blocks rendering.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;

use super::{Pace, SimLoop};
use crate::fault::{Fault, STEPPING, catching};

/// Handle on a [`SimLoop`] running on its own OS thread, taking commands over a channel.
pub struct Driver<L: SimLoop> {
    cmd_tx: mpsc::Sender<L::Command>,
    handle: Option<JoinHandle<()>>,
}

/// Prints whether the thread is still held, and leaves out the loop's command channel.
impl<L: SimLoop> std::fmt::Debug for Driver<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Driver")
            .field("thread", &self.handle.is_some())
            .finish_non_exhaustive()
    }
}

impl<L> Driver<L>
where
    L: SimLoop + Send + 'static,
    L::Command: Send + 'static,
{
    /// Spawns the thread and starts `sim` on it.
    ///
    /// `on_fault` runs on the sim thread if the loop panics.
    pub fn spawn(sim: L, on_fault: impl FnOnce(Fault) + Send + 'static) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        // Outside the loop. A catch per tick would sit in the hot path.
        let handle = std::thread::spawn(move || {
            if let Err(fault) = catching(STEPPING, || run(sim, &cmd_rx)) {
                log::error!("{fault}");
                on_fault(fault);
            }
        });
        Self {
            cmd_tx,
            handle: Some(handle),
        }
    }

    /// Sends a command to the loop. A loop that has stopped drops it.
    pub fn send(&mut self, cmd: L::Command) {
        drop(self.cmd_tx.send(cmd));
    }

    /// Does nothing, since the thread runs itself. A host calls it without knowing which driver it holds.
    pub fn update(&mut self, _dt: f64) {}

    /// Sends `cmd`, the loop's stop command, and joins the thread.
    ///
    /// The runners call it on drop, and it is the only way the thread is asked to stop.
    pub fn shutdown(&mut self, cmd: L::Command) {
        drop(self.cmd_tx.send(cmd));
        if let Some(handle) = self.handle.take() {
            drop(handle.join());
        }
    }
}

fn run<L: SimLoop + Send>(mut sim: L, cmd_rx: &mpsc::Receiver<L::Command>) {
    sim.start();
    loop {
        // The pump runs inside the pool. A kernel's parallel passes are otherwise injected from this
        // thread, which is not a worker, so each one parks the caller and wakes it again. Only the
        // pump moves, since the waits below would hold a worker while nothing is due.
        match rayon::scope(|_| sim.pump()) {
            Pace::Idle => {
                let Ok(cmd) = cmd_rx.recv() else { return };
                if sim.handle_command(cmd) {
                    return;
                }
            }
            Pace::After(wait) => match cmd_rx.recv_timeout(wait) {
                Ok(cmd) => {
                    if sim.handle_command(cmd) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            },
            Pace::Now => {}
        }

        while let Ok(cmd) = cmd_rx.try_recv() {
            if sim.handle_command(cmd) {
                return;
            }
        }
    }
}
