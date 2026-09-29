//! GPU tracks, each holding one live GPU run, and the interleaver that steps several of them on one device.
//!
//! The interleaver runs on the calling thread, since wgpu's error scopes belong to one thread. It visits the live
//! tracks in turn, and each visit collects a landed sample and submits the next command buffer of one track. A
//! command buffer holds the steps of one track only, at most [`GpuTrackDepth::steps_per_submission`] of them.
//!
//! A track records its work in the order a blocking run takes it: at each tick the actions due there, then the stats
//! passes of the sample due there, then the steps to the next tick anything is due at. A sample reads back while the
//! track records the steps after it, and those steps are thrown away with the state when the sample stops the run.
//! The stats passes of the next sample wait until the readback lands, as [`stepping::submit_slice`] requires.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use web_time::Instant;

use henad_compute::fault::{BUILDING, Fault, FaultKind, STEPPING, catching};
use henad_compute::gpu::fault::catching_on;
use henad_compute::gpu::{GpuContext, GpuSimState, StatsPoll, stepping};
use henad_core::action::Schedule;
use henad_core::explore::measure::Sampler;
use henad_core::explore::outcome::{PlannedRun, RunOutcome, RunStatus, StopReason};
use henad_models::registry::ModelState;

use super::{
    BatchEnd, ExecutionError, Executor, GpuTrackDepth, ReorderBuffer, RunRequest, RunSink, RunWatch, gpu_memory_budget,
};
use crate::cursor::{OutcomeParts, RunEnd, RunFailure, failed_build_outcome, milliseconds, refusal_note, run_outcome};

/// Runs `requests` on the GPU tracks of `executor`, and commits the outcomes to `sink` in request order.
///
/// A run is built once a track is free and the device demand of the live runs and its own fits the GPU memory
/// budget. A run that fits beside no other run runs alone. A build that runs out of device memory while other runs
/// are live is queued again, and waits for a live run to end. The track count drops by one.
///
/// A fault while a track records its work ends that track alone. A fault the device reports outside every track's
/// error scopes ends every live track. A lost device, or a failed wait on it, ends the batch as
/// [`BatchEnd::DeviceLost`], and a run that failed on the GPU is then left without an outcome.
///
/// # Errors
///
/// Returns [`ExecutionError::Sink`] when `sink` refuses a run.
pub(super) fn run_on_tracks(
    executor: &Executor<'_>,
    ctx: &GpuContext,
    requests: &[RunRequest<'_>],
    sink: &mut dyn RunSink,
) -> Result<BatchEnd, ExecutionError> {
    let mut interleaver = Interleaver {
        executor,
        ctx,
        requests,
        unbuilt_requests: (0..requests.len()).collect(),
        tracks: Vec::new(),
        track_cap: executor.layout.gpu_tracks.max(1),
        gpu_memory_budget: gpu_memory_budget(executor.gpu_memory, ctx),
        admission_held: false,
        submissions: VecDeque::new(),
        failed_builds: Vec::new(),
        reorder: ReorderBuffer::default(),
    };
    loop {
        if !executor.control.proceed() {
            return Ok(BatchEnd::Aborted);
        }
        if let Some(end) = interleaver.round(sink)? {
            return Ok(end);
        }
    }
}

/// Live tracks of one batch, and the requests still to build.
struct Interleaver<'x> {
    executor: &'x Executor<'x>,
    ctx: &'x GpuContext,
    requests: &'x [RunRequest<'x>],
    /// Indexes of the requests not built yet, in the order they are built.
    unbuilt_requests: VecDeque<usize>,
    tracks: Vec<GpuTrack>,
    /// Most tracks alive at once.
    track_cap: usize,
    /// Bytes of device memory the live runs can hold together.
    gpu_memory_budget: u64,
    /// Whether admission waits for a live track to end, set when a build runs out of device memory.
    admission_held: bool,
    /// Command buffers of every track the device has not finished, oldest first.
    submissions: VecDeque<Submission>,
    /// Runs whose build failed this round.
    failed_builds: Vec<EndedRun>,
    /// Outcomes waiting to be committed in request order.
    reorder: ReorderBuffer<RunOutcome>,
}

/// A run that ended this round, with its outcome not yet handed out.
struct EndedRun {
    /// Index of the run's request in its batch.
    request_index: usize,
    outcome: RunOutcome,
    watch: Option<RunWatch>,
}

/// A command buffer on the device.
struct Submission {
    index: wgpu::SubmissionIndex,
    /// Set once the device has finished the buffer.
    done: Arc<AtomicBool>,
}

impl Interleaver<'_> {
    /// Builds the runs that fit, visits every live track once and commits the runs that finished.
    ///
    /// Returns the end of the batch once it has one. A round in which no track moved waits for the device to finish
    /// its oldest command buffer.
    fn round(&mut self, sink: &mut dyn RunSink) -> Result<Option<BatchEnd>, ExecutionError> {
        let mut moved = self.admit();
        let started = Instant::now();
        if self.poll_device().is_err() || self.ctx.is_lost() {
            return Ok(Some(BatchEnd::DeviceLost));
        }
        for position in 0..self.tracks.len() {
            moved |= self.visit(position);
        }
        if let Some(fault) = self.ctx.faults.take() {
            self.fail_every_track(&RunFailure::from(fault), None);
        }
        if !moved && self.wait_for_oldest().is_err() {
            return Ok(Some(BatchEnd::DeviceLost));
        }
        self.charge(started.elapsed());
        if !self.hand_out_finished(sink) {
            return Ok(Some(BatchEnd::DeviceLost));
        }
        while let Some(outcome) = self.reorder.pop_ready() {
            self.executor.commit(sink, outcome)?;
        }
        if self.unbuilt_requests.is_empty() && self.tracks.is_empty() {
            debug_assert_eq!(self.reorder.handed_out(), self.requests.len(), "every run is committed");
            return Ok(Some(BatchEnd::Complete));
        }
        Ok(None)
    }

    /// Builds queued runs while a track is free and the next run fits the memory budget, and returns whether any
    /// build was tried.
    ///
    /// Nothing is built while admission is held after an out-of-memory build.
    fn admit(&mut self) -> bool {
        let mut tried = false;
        while !self.admission_held && self.tracks.len() < self.track_cap {
            let Some(&request_index) = self.unbuilt_requests.front() else {
                break;
            };
            let request = &self.requests[request_index];
            let demand_bytes = self
                .executor
                .entry
                .demand(request.params)
                .map_or(0, |demand| demand.bytes());
            let live_bytes: u64 = self.tracks.iter().map(|track| track.demand_bytes).sum();
            if !self.tracks.is_empty() && live_bytes.saturating_add(demand_bytes) > self.gpu_memory_budget {
                break;
            }
            self.unbuilt_requests.pop_front();
            tried = true;
            match GpuTrack::build(self.executor, request_index, request, demand_bytes) {
                Ok(track) => self.tracks.push(track),
                Err(failed) if failed.fault.is_out_of_memory() && !self.tracks.is_empty() => {
                    self.unbuilt_requests.push_front(request_index);
                    self.track_cap = self.track_cap.saturating_sub(1).max(1);
                    self.admission_held = true;
                }
                Err(failed) => {
                    let failure = RunFailure::from(failed.fault);
                    let measure = &self.executor.measure;
                    let outcome = failed_build_outcome(request.run, request.run_key, measure, failed.build_ms, failure);
                    self.failed_builds.push(EndedRun {
                        request_index,
                        outcome,
                        watch: failed.watch,
                    });
                }
            }
        }
        tried
    }

    /// Runs the device's callbacks, marking the command buffers it finished, and drops them from the oldest.
    ///
    /// # Errors
    ///
    /// Returns the fault of a poll that failed.
    fn poll_device(&mut self) -> Result<(), Fault> {
        let polled = catching(STEPPING, || self.ctx.device.poll(wgpu::PollType::Poll));
        polled.and_then(|poll| poll.map(drop).map_err(poll_fault))?;
        while self.submissions.front().is_some_and(Submission::is_done) {
            self.submissions.pop_front();
        }
        Ok(())
    }

    /// Collects the sample of track `position` and records its next command buffer, and returns whether the track
    /// moved.
    ///
    /// A fault the device reports outside every error scope ends every live track. This track also drops the sample
    /// its new command buffer reads back.
    fn visit(&mut self, position: usize) -> bool {
        let track = &mut self.tracks[position];
        if track.end.is_some() {
            return false;
        }
        let landed = match track.check_sample(self.ctx) {
            SampleCheck::Pending => false,
            SampleCheck::Landed => true,
            SampleCheck::Ended(end) => {
                track.end = Some(end);
                return true;
            }
        };
        match track.record(self.ctx, self.executor.track_depth, &mut self.submissions) {
            Ok(VisitWork::Nothing) => landed,
            Ok(VisitWork::Submitted { sampled }) => {
                if let Some(fault) = self.ctx.faults.take() {
                    self.fail_every_track(&RunFailure::from(fault), sampled.then_some(position));
                }
                true
            }
            Err(fault) => {
                track.fail(fault.into(), false);
                true
            }
        }
    }

    /// Ends every live track on `failure`, a fault that no error scope traced to one track.
    ///
    /// Each run fails however it ends, a run that ended earlier this round included. The track at `sampled_position`
    /// drops the sample its latest command buffer read back.
    fn fail_every_track(&mut self, failure: &RunFailure, sampled_position: Option<usize>) {
        for (position, track) in self.tracks.iter_mut().enumerate() {
            track.untraced_failure.get_or_insert_with(|| failure.clone());
            if track.end.is_none() {
                track.fail(failure.clone(), sampled_position == Some(position));
            }
        }
    }

    /// Blocks until the device finishes the oldest command buffer it has not finished.
    ///
    /// A buffer the device finished since the last poll lets the next round move, and nothing waits. With no buffer
    /// on the device, the thread yields instead.
    ///
    /// # Errors
    ///
    /// Returns the fault of a wait that failed.
    fn wait_for_oldest(&self) -> Result<(), Fault> {
        let Some(oldest) = self.submissions.front() else {
            std::thread::yield_now();
            return Ok(());
        };
        if oldest.is_done() {
            return Ok(());
        }
        let (ctx, submission_index) = (self.ctx, oldest.index.clone());
        catching(STEPPING, || stepping::await_submission(ctx, submission_index))?
    }

    /// Adds `elapsed` to the wall time of every live track, and ends a track past its timeout.
    fn charge(&mut self, elapsed: Duration) {
        let timeout = self.executor.timeout;
        for track in &mut self.tracks {
            track.wall += elapsed;
            let recording = track.end.is_none() && track.failure.is_none();
            if recording && timeout.is_some_and(|timeout| track.wall >= timeout) {
                track.end = Some(RunEnd::TimedOut);
            }
        }
    }

    /// Removes the tracks that ended this round, and hands their outcomes and those of the failed builds to `sink`
    /// and the reorder buffer.
    ///
    /// A removed track releases admission held after an out-of-memory build. Returns `false`, handing out nothing,
    /// when a run failed on the GPU and the device turns out to be lost.
    fn hand_out_finished(&mut self, sink: &mut dyn RunSink) -> bool {
        let mut ended = std::mem::take(&mut self.failed_builds);
        let mut position = 0;
        while position < self.tracks.len() {
            let Some(end) = self.tracks[position].end.take() else {
                position += 1;
                continue;
            };
            let track = self.tracks.remove(position);
            ended.push(track.finish(end, self.executor.timeout));
            self.admission_held = false;
        }
        if ended.iter().any(|run| run.outcome.status == RunStatus::GpuError) && self.device_lost() {
            return false;
        }
        for run in ended {
            if let Some(watch) = run.watch {
                watch.finish();
            }
            sink.finished(&run.outcome);
            self.reorder.insert(run.request_index, run.outcome);
        }
        true
    }

    /// Returns whether the device is lost, once it has finished every command buffer.
    ///
    /// Note that after `Device::destroy`, builds and readbacks fail before the device reports its loss. It reports the
    /// loss only once a poll finds its queue empty, and the wait is that poll.
    fn device_lost(&self) -> bool {
        let drained = catching(STEPPING, || self.ctx.device.poll(wgpu::PollType::wait_indefinitely()));
        !matches!(drained, Ok(Ok(_))) || self.ctx.is_lost()
    }
}

impl Submission {
    fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

/// One GPU run, recorded a command buffer at a time.
struct GpuTrack {
    /// Index of the run's request in its batch.
    request_index: usize,
    run: PlannedRun,
    run_key: u64,
    state: Box<dyn GpuSimState>,
    schedule: Schedule,
    /// Bytes of device memory the run holds.
    demand_bytes: u64,
    sampler: Sampler,
    /// Tick the recorded steps reach.
    tick: u64,
    /// Whether the actions due at `tick` are recorded.
    actions_recorded: bool,
    /// Next sampled tick whose stats passes are not recorded yet, `None` once the last one is.
    next_sample: Option<u64>,
    /// Tick of the sample whose readback is in flight.
    readback_tick: Option<u64>,
    /// Tick of the latest sample to land, 0 before the first.
    landed_tick: u64,
    /// Population at the latest sample.
    population: u64,
    /// Tick and note of each action the model refused, in the order they were due.
    refusals: Vec<(u64, String)>,
    /// Done flag of each command buffer of the track the device might still hold, oldest first.
    done_flags: VecDeque<Arc<AtomicBool>>,
    build_ms: f64,
    /// Time the run has been live, the pauses of its batch left out.
    wall: Duration,
    /// Fault that ended the recording, held while the sample before it reads back.
    failure: Option<RunFailure>,
    /// Fault that no error scope traced to one track, raised while the run was live. It fails the run even at a sample
    /// that stops it.
    untraced_failure: Option<RunFailure>,
    /// End of the run, set once it has one.
    end: Option<RunEnd>,
    watch: Option<RunWatch>,
}

/// Fault of a run's build.
struct BuildFailure {
    fault: Fault,
    build_ms: f64,
    watch: Option<RunWatch>,
}

/// State of a track's sample after one visit.
enum SampleCheck {
    /// No sample landed.
    Pending,
    /// A sample landed and the run goes on.
    Landed,
    /// The run ended.
    Ended(RunEnd),
}

/// Work one visit recorded.
enum VisitWork {
    /// The track waits on the device or on its readback, has recorded its final sample, or its recording failed.
    Nothing,
    /// Actions or a command buffer. `sampled` says whether the buffer held the stats passes of a sample.
    Submitted { sampled: bool },
}

impl GpuTrack {
    /// Builds the model of `request`, the request at `request_index` of its batch, with the request's seed, and
    /// times the build.
    ///
    /// The run is listed in the executor's active runs from before its build.
    fn build(
        executor: &Executor<'_>,
        request_index: usize,
        request: &RunRequest<'_>,
        demand_bytes: u64,
    ) -> Result<Self, BuildFailure> {
        let watch = executor
            .active_runs
            .as_ref()
            .map(|active_runs| active_runs.watch(request.run, executor.measure.total()));
        let started = Instant::now();
        let built = (executor.entry.create)(request.params, Some(request.run.seed)).and_then(gpu_state);
        let build_ms = milliseconds(started.elapsed());
        let state = match built {
            Ok(state) => state,
            Err(fault) => {
                return Err(BuildFailure { fault, build_ms, watch });
            }
        };
        Ok(Self {
            request_index,
            run: request.run,
            run_key: request.run_key,
            state,
            schedule: request.schedule.clone(),
            demand_bytes,
            sampler: Sampler::new(Arc::clone(&executor.measure)),
            tick: 0,
            actions_recorded: false,
            next_sample: Some(executor.measure.first_sample()),
            readback_tick: None,
            landed_tick: 0,
            population: 0,
            refusals: Vec::new(),
            done_flags: VecDeque::new(),
            build_ms,
            wall: Duration::ZERO,
            failure: None,
            untraced_failure: None,
            end: None,
            watch,
        })
    }

    /// Collects the sample reading back once it lands.
    ///
    /// A track whose recording failed ends once the sample before its fault has landed. A sample that stops the
    /// run, or is its last, ends it as a blocking run would have, and a fault an error scope traced to the steps past
    /// it is dropped.
    fn check_sample(&mut self, ctx: &GpuContext) -> SampleCheck {
        let Some(tick) = self.readback_tick else {
            return self.failure.take().map_or(SampleCheck::Pending, |failure| {
                SampleCheck::Ended(RunEnd::Failed(failure))
            });
        };
        let state = &mut self.state;
        let sample = match catching(STEPPING, || state.poll_stats_readback(&ctx.device, false)) {
            Ok(StatsPoll::Pending) => return SampleCheck::Pending,
            Ok(StatsPoll::Failed) => {
                let failure = self.failure.take().unwrap_or_else(|| RunFailure {
                    status: RunStatus::GpuError,
                    note: format!("stats readback failed at tick {tick}"),
                });
                return SampleCheck::Ended(RunEnd::Failed(failure));
            }
            Ok(StatsPoll::Landed) => catching(STEPPING, || (state.stats(), state.population())),
            Err(fault) => Err(fault),
        };
        let (stats, population) = match sample {
            Ok(sample) => sample,
            Err(fault) => {
                let failure = self.failure.take().unwrap_or_else(|| fault.into());
                return SampleCheck::Ended(RunEnd::Failed(failure));
            }
        };
        self.readback_tick = None;
        self.population = population;
        self.landed_tick = tick;
        match self.sampler.push(tick, &stats) {
            Err(error) => SampleCheck::Ended(RunEnd::Failed(error.into())),
            Ok(true) => SampleCheck::Ended(RunEnd::Stopped(StopReason::Condition)),
            Ok(false) if tick == self.sampler.plan().total() => SampleCheck::Ended(RunEnd::Stopped(StopReason::Steps)),
            Ok(false) => match self.failure.take() {
                Some(failure) => SampleCheck::Ended(RunEnd::Failed(failure)),
                None => SampleCheck::Landed,
            },
        }
    }

    /// Records the actions due at the track's tick, then one command buffer: the stats passes of the sample due
    /// there, or the steps to the next tick anything is due at.
    ///
    /// A command buffer that reaches a sampled tick with nothing due there and no readback in flight holds the stats
    /// passes of that sample too. Each recorded buffer goes on `submissions`.
    ///
    /// # Errors
    ///
    /// Returns the fault an action or a command buffer raised.
    fn record(
        &mut self,
        ctx: &GpuContext,
        depth: GpuTrackDepth,
        submissions: &mut VecDeque<Submission>,
    ) -> Result<VisitWork, Fault> {
        if self.failure.is_some() {
            return Ok(VisitWork::Nothing);
        }
        while self.done_flags.front().is_some_and(|done| done.load(Ordering::Acquire)) {
            self.done_flags.pop_front();
        }
        if self.done_flags.len() >= depth.submissions_per_track {
            return Ok(VisitWork::Nothing);
        }
        let mut work = VisitWork::Nothing;
        if !self.actions_recorded {
            self.actions_recorded = true;
            if self.schedule.due(self.tick).next().is_some() {
                let (state, schedule) = (&mut self.state, &self.schedule);
                let refused = catching_on(ctx, STEPPING, || stepping::run_due(&mut **state, ctx, schedule))?;
                self.refusals
                    .extend(refused.into_iter().map(|action| (action.tick, refusal_note(action))));
                work = VisitWork::Submitted { sampled: false };
            }
        }
        let Some(sample_tick) = self.next_sample else {
            return Ok(work);
        };
        let (count, sampled) = if sample_tick == self.tick {
            if self.readback_tick.is_some() {
                return Ok(work);
            }
            (0, true)
        } else {
            let mut boundary = sample_tick.min(self.tick + u64::from(depth.steps_per_submission));
            if let Some(due) = self.schedule.next_due_after(self.tick) {
                boundary = boundary.min(due);
            }
            let actions_due = self.schedule.due(boundary).next().is_some();
            let sampled = boundary == sample_tick && !actions_due && self.readback_tick.is_none();
            ((boundary - self.tick) as u32, sampled)
        };
        debug_assert!(
            !sampled || !self.state.stats_readback_pending(),
            "run {} recorded a sample while an earlier one reads back, and its copy would be skipped",
            self.run.run_id
        );
        let state = &mut self.state;
        let submission_index = catching_on(ctx, STEPPING, || {
            stepping::submit_slice(&mut **state, ctx, count, sampled)
        })?;
        let done = Arc::new(AtomicBool::new(false));
        let callback_done = Arc::clone(&done);
        ctx.queue
            .on_submitted_work_done(move || callback_done.store(true, Ordering::Release));
        self.done_flags.push_back(Arc::clone(&done));
        submissions.push_back(Submission {
            index: submission_index,
            done,
        });

        if count > 0 {
            self.tick += u64::from(count);
            self.actions_recorded = self.schedule.due(self.tick).next().is_none();
        }
        if sampled {
            self.readback_tick = Some(self.tick);
            self.next_sample = self.sampler.plan().next_sample(self.tick);
        }
        if let Some(watch) = &self.watch {
            watch.reach(self.tick);
        }
        Ok(VisitWork::Submitted { sampled })
    }

    /// Ends the track's recording on `failure`, keeping the first failure. With `discard_readback`, the sample
    /// reading back is dropped.
    fn fail(&mut self, failure: RunFailure, discard_readback: bool) {
        if discard_readback {
            self.readback_tick = None;
        }
        self.failure.get_or_insert(failure);
    }

    /// Returns the run with its outcome for `end`. `timeout` is the limit a timed-out run's note quotes.
    ///
    /// The run ends at the tick of its latest sample to land. An action refused past that tick was recorded after the
    /// run's end, and is left out.
    fn finish(self, end: RunEnd, timeout: Option<Duration>) -> EndedRun {
        let end = match (end, self.untraced_failure) {
            (RunEnd::Failed(failure), _) | (_, Some(failure)) => RunEnd::Failed(failure),
            (end, None) => end,
        };
        let ticks = self.landed_tick;
        let refusals = self
            .refusals
            .into_iter()
            .filter(|&(tick, _)| tick <= ticks)
            .map(|(_, note)| note)
            .collect();
        let parts = OutcomeParts {
            sampler: self.sampler,
            ticks,
            population: self.population,
            build_ms: self.build_ms,
            wall: self.wall,
            timeout,
            refusals,
        };
        EndedRun {
            request_index: self.request_index,
            outcome: run_outcome(self.run, self.run_key, end, parts),
            watch: self.watch,
        }
    }
}

/// Returns the GPU state of `state`, refusing a CPU model.
fn gpu_state(state: ModelState) -> Result<Box<dyn GpuSimState>, Fault> {
    match state {
        ModelState::Gpu(state) => Ok(state),
        ModelState::Cpu(_) => Err(Fault::refused(BUILDING, "a CPU model cannot run on a GPU track")),
    }
}

fn poll_fault(error: wgpu::PollError) -> Fault {
    Fault {
        during: STEPPING,
        kind: FaultKind::Poll(error),
    }
}
