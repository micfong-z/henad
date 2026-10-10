//! The native driver, which runs a sim loop on its own OS thread so stepping never blocks rendering.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{DetachedRunner, Pace, RunnerShutdown, SimLoop};
use crate::fault::{Fault, STEPPING, catching};

/// First wait between two looks at a stopping thread. Each later wait doubles, up to [`STOP_POLL_MAX`].
const STOP_POLL_MIN: Duration = Duration::from_micros(50);

/// Longest wait between two looks at a stopping thread.
const STOP_POLL_MAX: Duration = Duration::from_millis(1);

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
    /// Spawns the thread and starts `sim` on it. Each pump runs on a worker of the global rayon pool.
    ///
    /// `on_fault` runs on the sim thread if the loop panics.
    pub fn spawn(sim: L, on_fault: impl FnOnce(Fault) + Send + 'static) -> Self {
        // The pump runs inside the pool. A kernel's parallel passes are otherwise injected from this thread, which is
        // not a worker, so each one parks the caller and wakes it again. Only the pump moves, since the waits would
        // hold a worker while nothing is due.
        Self::spawn_with(
            sim,
            on_fault,
            |sim| sim.start(),
            |sim| rayon::scope(|_| sim.pump()),
            |sim, cmd| sim.handle_command(cmd),
        )
    }

    /// Spawns the thread and starts `sim` on it. The loop's start, each pump and each command run on a worker of
    /// `pool`, and a model step that never returns holds workers of `pool` alone.
    ///
    /// `on_fault` runs on the sim thread if the loop panics.
    pub fn spawn_in(pool: Arc<rayon::ThreadPool>, sim: L, on_fault: impl FnOnce(Fault) + Send + 'static) -> Self {
        let start_pool = Arc::clone(&pool);
        let command_pool = Arc::clone(&pool);
        Self::spawn_with(
            sim,
            on_fault,
            move |sim| start_pool.install(|| sim.start()),
            move |sim| pool.install(|| sim.pump()),
            move |sim, cmd| command_pool.install(|| sim.handle_command(cmd)),
        )
    }

    /// Spawns the thread and runs `sim` on it. `start`, `pump` and `handle` make its calls of [`SimLoop::start`],
    /// [`SimLoop::pump`] and [`SimLoop::handle_command`].
    fn spawn_with(
        sim: L,
        on_fault: impl FnOnce(Fault) + Send + 'static,
        start: impl FnOnce(&mut L) + Send + 'static,
        pump: impl FnMut(&mut L) -> Pace + Send + 'static,
        handle: impl FnMut(&mut L, L::Command) -> bool + Send + 'static,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        // Outside the loop. A catch per tick would sit in the hot path.
        let handle = std::thread::spawn(move || {
            if let Err(fault) = catching(STEPPING, || run(sim, &cmd_rx, start, pump, handle)) {
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

    /// Sends `cmd`, the loop's stop command, and joins the thread once it stops, or detaches it after `timeout`.
    ///
    /// The runners call it from `stop` and on drop, and it is the only way the thread is asked to stop. A second call
    /// returns [`RunnerShutdown::Joined`] and sends nothing.
    pub fn shutdown(&mut self, cmd: L::Command, timeout: Duration) -> RunnerShutdown {
        let Some(handle) = self.handle.take() else {
            return RunnerShutdown::Joined;
        };
        drop(self.cmd_tx.send(cmd));
        let deadline = Instant::now() + timeout;
        let mut wait = STOP_POLL_MIN;
        while !handle.is_finished() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return RunnerShutdown::Detached(DetachedRunner { handle });
            }
            std::thread::sleep(wait.min(left));
            wait = (wait * 2).min(STOP_POLL_MAX);
        }
        // Already finished. This never actually waits.
        drop(handle.join());
        RunnerShutdown::Joined
    }
}

fn run<L: SimLoop>(
    mut sim: L,
    cmd_rx: &mpsc::Receiver<L::Command>,
    start: impl FnOnce(&mut L),
    mut pump: impl FnMut(&mut L) -> Pace,
    mut handle: impl FnMut(&mut L, L::Command) -> bool,
) {
    start(&mut sim);
    loop {
        match pump(&mut sim) {
            Pace::Idle => {
                let Ok(cmd) = cmd_rx.recv() else { return };
                if handle(&mut sim, cmd) {
                    return;
                }
            }
            Pace::After(wait) => match cmd_rx.recv_timeout(wait) {
                Ok(cmd) => {
                    if handle(&mut sim, cmd) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            },
            Pace::Now => {}
        }

        while let Ok(cmd) = cmd_rx.try_recv() {
            if handle(&mut sim, cmd) {
                return;
            }
        }
    }
}
