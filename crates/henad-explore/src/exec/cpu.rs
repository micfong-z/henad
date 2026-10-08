//! CPU lanes, each stepping one run at a time on its own thread pool.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use henad_core::explore::outcome::RunOutcome;

use super::{BatchEnd, ExecutionError, Executor, Placement, ReorderBuffer, RunRequest, RunSink, SweepControl};

/// Runs `requests` in one lane per pool in `pools`, and commits the outcomes in request order.
///
/// Each lane takes the next request not yet taken, so a slow run never holds up the other lanes. A panic on a lane's
/// thread or in `sink` aborts the control, and the other lanes stop within a slice. A lane's panic ends the batch
/// with [`ExecutionError::LanePanicked`], and a panic in `sink` unwinds to the caller.
pub(super) fn run_in_lanes(
    executor: &Executor<'_>,
    pools: &[rayon::ThreadPool],
    requests: &[RunRequest<'_>],
    sink: &mut dyn RunSink,
) -> Result<BatchEnd, ExecutionError> {
    let next_request = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel::<(usize, RunOutcome)>();
    thread::scope(|scope| {
        let mut lanes = Vec::with_capacity(pools.len());
        for (lane, pool) in pools.iter().enumerate() {
            let sender = sender.clone();
            let next_request = &next_request;
            let spawned = thread::Builder::new()
                .name(format!("henad-lane-{lane}"))
                .spawn_scoped(scope, move || {
                    let _abort_on_panic = AbortOnPanic(&executor.control);
                    while executor.control.proceed() {
                        let index = next_request.fetch_add(1, Ordering::Relaxed);
                        let Some(request) = requests.get(index) else {
                            break;
                        };
                        let Some(outcome) = executor.drive_in(Placement::LanePool(pool), request) else {
                            break;
                        };
                        if sender.send((index, outcome)).is_err() {
                            break;
                        }
                    }
                });
            match spawned {
                Ok(handle) => lanes.push(handle),
                Err(error) => {
                    executor.control.abort();
                    return Err(ExecutionError::Spawn(error));
                }
            }
        }
        drop(sender);

        let _abort_on_panic = AbortOnPanic(&executor.control);
        let mut reorder = ReorderBuffer::default();
        let mut refused = None;
        for (index, outcome) in &receiver {
            sink.finished(&outcome);
            reorder.insert(index, outcome);
            while let Some(ready) = reorder.pop_ready() {
                if let Err(error) = executor.commit(sink, ready) {
                    refused = Some(error);
                    break;
                }
            }
            if refused.is_some() {
                break;
            }
        }
        let mut lost_lane = false;
        // A joined lane returns its panic here. The scope raises a panic only for a thread it joins itself.
        for lane in lanes {
            lost_lane |= lane.join().is_err();
        }
        if let Some(error) = refused {
            Err(error)
        } else if lost_lane {
            Err(ExecutionError::LanePanicked)
        } else if reorder.handed_out() == requests.len() {
            Ok(BatchEnd::Complete)
        } else {
            Ok(BatchEnd::Aborted)
        }
    })
}

/// Guard that aborts its control when dropped by a panic unwinding.
struct AbortOnPanic<'c>(&'c SweepControl);

impl Drop for AbortOnPanic<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.abort();
        }
    }
}
