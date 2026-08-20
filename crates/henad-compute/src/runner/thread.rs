//! Drives a loop on an OS thread of its own, so stepping never blocks rendering.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;

use super::{Pace, SimLoop};
use crate::fault::{Fault, STEPPING, catching};

pub struct Driver<L: SimLoop> {
    cmd_tx: mpsc::Sender<L::Command>,
    handle: Option<JoinHandle<()>>,
}

impl<L> Driver<L>
where
    L: SimLoop + Send + 'static,
    L::Command: Send + 'static,
{
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

    pub fn send(&mut self, cmd: L::Command) {
        drop(self.cmd_tx.send(cmd));
    }

    /// The thread runs itself. Present so a host can call it without knowing which driver it has.
    pub fn update(&mut self, _dt: f64) {}

    /// Sent on drop, and the only way the thread is asked to stop.
    pub fn shutdown(&mut self, cmd: L::Command) {
        drop(self.cmd_tx.send(cmd));
        if let Some(handle) = self.handle.take() {
            drop(handle.join());
        }
    }
}

fn run<L: SimLoop>(mut sim: L, cmd_rx: &mpsc::Receiver<L::Command>) {
    sim.start();
    loop {
        match sim.pump() {
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
