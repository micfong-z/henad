//! Progress of a sweep or search, reported to the host as events.
//!
//! The library never prints. A host renders the events as it likes, such as a line on a terminal, JSON lines or a
//! panel.

use std::time::Duration;

use web_time::Instant;

use henad_core::explore::outcome::{RunOutcome, RunStatus};

use crate::search_run::SearchUpdate;
use crate::sweep::{SweepOutline, SweepReport, SweepWarning};

/// Shortest time between two [`ProgressEvent::Progressed`] events.
pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// Event in the progress of a sweep.
#[derive(Debug, Clone, Copy)]
pub enum ProgressEvent<'a> {
    /// The sweep is planned and probed. A dry run ends here.
    Planned(&'a SweepOutline),
    /// A warning. The sweep still runs, but likely not as intended.
    Warned(&'a SweepWarning),
    /// A run was written. Runs arrive in plan order, and a search's runs in the order that it requests them.
    RunCommitted(&'a RunOutcome),
    /// Runs have finished since the last update. Sent at most once per [`PROGRESS_INTERVAL`].
    Progressed(ProgressUpdate),
    /// A search was told the evaluations of one batch. Batches arrive in order, each after its runs.
    SearchBatchTold(&'a SearchUpdate),
    /// The sweep ended without an error.
    Ended(&'a SweepReport),
}

/// Receiver of a sweep's progress.
pub trait Progress {
    /// Receives `event` as it happens.
    fn report(&mut self, event: &ProgressEvent<'_>);
}

/// Progress that reports nowhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn report(&mut self, _event: &ProgressEvent<'_>) {}
}

/// Runs finished so far, and the time left at the pace so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressUpdate {
    /// Number of runs finished so far. A run counts once it finishes, before it is written.
    pub done: u64,
    /// Number of runs this session executes.
    pub total: u64,
    /// Number of finished runs that ended on a fault or a timeout.
    pub failed: u64,
    /// Time since the [`ProgressMeter`] started. The meter starts before the first run is built.
    pub elapsed: Duration,
    /// Time left at the mean pace of the finished runs, `None` before the first run finishes.
    ///
    /// A time too long for a [`Duration`] is `None` as well.
    pub remaining: Option<Duration>,
}

impl ProgressUpdate {
    /// Returns the update after `done` of `total` runs in `elapsed`, `failed` of them on a fault or a timeout.
    pub fn new(done: u64, total: u64, failed: u64, elapsed: Duration) -> Self {
        let remaining = (done > 0)
            .then(|| elapsed.as_secs_f64() / done as f64 * total.saturating_sub(done) as f64)
            .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok());
        Self {
            done,
            total,
            failed,
            elapsed,
            remaining,
        }
    }
}

/// Counter of finished runs that yields an update at most once per [`PROGRESS_INTERVAL`].
#[derive(Debug, Clone)]
pub struct ProgressMeter {
    total: u64,
    done: u64,
    failed: u64,
    started: Instant,
    last_update: Instant,
}

impl ProgressMeter {
    /// Returns a meter for `total` runs, started now.
    pub fn new(total: u64) -> Self {
        let now = Instant::now();
        Self {
            total,
            done: 0,
            failed: 0,
            started: now,
            last_update: now,
        }
    }

    /// Counts a finished run with `status`. Returns an update once [`PROGRESS_INTERVAL`] has passed since the last
    /// update.
    pub fn record_finished_run(&mut self, status: RunStatus) -> Option<ProgressUpdate> {
        self.tally(status);
        let now = Instant::now();
        if now.duration_since(self.last_update) < PROGRESS_INTERVAL {
            return None;
        }
        self.last_update = now;
        Some(self.update())
    }

    fn tally(&mut self, status: RunStatus) {
        self.done += 1;
        if status.is_failure() {
            self.failed += 1;
        }
    }

    /// Returns the update for the runs counted so far.
    pub fn update(&self) -> ProgressUpdate {
        ProgressUpdate::new(self.done, self.total, self.failed, self.started.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use henad_core::explore::outcome::RunStatus;

    use super::{PROGRESS_INTERVAL, ProgressMeter, ProgressUpdate};

    #[test]
    fn the_remaining_time_scales_the_elapsed_time_by_the_runs_left() {
        let update = ProgressUpdate::new(4, 10, 1, Duration::from_secs(8));
        assert_eq!(update.remaining, Some(Duration::from_secs(12)));
        assert_eq!(update.elapsed, Duration::from_secs(8));
        assert_eq!(ProgressUpdate::new(0, 10, 0, Duration::from_secs(3)).remaining, None);
        assert_eq!(
            ProgressUpdate::new(10, 10, 0, Duration::from_secs(3)).remaining,
            Some(Duration::ZERO)
        );
        assert_eq!(
            ProgressUpdate::new(1, u64::MAX, 0, Duration::from_secs(2)).remaining,
            None,
            "a time too long for a duration reads as none"
        );
    }

    #[test]
    fn the_meter_updates_at_most_once_per_interval() {
        let mut meter = ProgressMeter::new(3);
        assert_eq!(
            meter.record_finished_run(RunStatus::Ok),
            None,
            "the interval has not passed"
        );
        meter.last_update = meter
            .last_update
            .checked_sub(PROGRESS_INTERVAL)
            .expect("the clock has run for longer than one interval");
        let update = meter
            .record_finished_run(RunStatus::Panicked)
            .expect("the interval has passed");
        assert_eq!((update.done, update.total, update.failed), (2, 3, 1));
        assert_eq!(
            meter.record_finished_run(RunStatus::NonFinite),
            None,
            "the interval starts again"
        );
        assert_eq!((meter.update().done, meter.update().failed), (3, 1));
    }
}
