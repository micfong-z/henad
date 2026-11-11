//! Tests of the CPU runner's pacing, publishing, layout switches and schedules.

use crate::cpu::sim_thread::{SimCommand, SimThread, WakeFn};
use crate::cpu::tests::support::wait_until;
use crate::fault::{FaultSink, STEPPING};
use crate::snapshot::Snapshot;
use henad_core::action::{Schedule, Scheduled};
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::StatEntry;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Counts its steps as its tick, reports the tick as its one stat, and records the tick each action runs at.
#[derive(Default)]
struct Counter {
    ticks: Arc<AtomicU64>,
    fired: Arc<Mutex<Vec<u64>>>,
}

impl SimState for Counter {
    fn step(&mut self) {
        self.ticks.fetch_add(1, Ordering::Relaxed);
    }
    fn tick(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }
    fn stats(&self) -> Vec<StatEntry> {
        vec![StatEntry::new(
            "Tick",
            henad_core::view::StatValue::Scalar(self.tick() as f64),
            [0; 4],
        )]
    }
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }
    fn act(&mut self, _index: usize) -> bool {
        self.fired.lock().expect("action log").push(self.tick());
        true
    }
    fn population(&self) -> u64 {
        0
    }
    fn heap_bytes(&self) -> usize {
        0
    }
}

/// Returns a thread over a [`Counter`] capped at `target_tps`, with its tick and its action log.
fn counter(target_tps: f64, wake: Option<WakeFn>) -> (SimThread, Arc<AtomicU64>, Arc<Mutex<Vec<u64>>>) {
    let state = Counter::default();
    let (ticks, fired) = (Arc::clone(&state.ticks), Arc::clone(&state.fired));
    let thread = SimThread::new(Box::new(state), target_tps, wake, FaultSink::new());
    (thread, ticks, fired)
}

/// Counts its steps as its tick, records every layout switch the loop passes on, and counts its relaxes.
struct Relaxes {
    ticks: u64,
    switches: Arc<Mutex<Vec<bool>>>,
    relaxes: Arc<AtomicU64>,
}

impl Relaxes {
    /// Returns a thread over a new state, with its log of layout switches and its count of relaxes.
    fn spawn() -> (SimThread, Arc<Mutex<Vec<bool>>>, Arc<AtomicU64>) {
        let switches = Arc::new(Mutex::new(Vec::new()));
        let relaxes = Arc::new(AtomicU64::new(0));
        let state = Self {
            ticks: 0,
            switches: Arc::clone(&switches),
            relaxes: Arc::clone(&relaxes),
        };
        let thread = SimThread::new(Box::new(state), 50.0, None, FaultSink::new());
        (thread, switches, relaxes)
    }
}

impl SimState for Relaxes {
    fn step(&mut self) {
        self.ticks += 1;
    }
    fn tick(&self) -> u64 {
        self.ticks
    }
    fn stats(&self) -> Vec<StatEntry> {
        Vec::new()
    }
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }
    fn set_layout(&mut self, on: bool, _budget_ms: f32) -> bool {
        // Logged under the lock, so a test that sees the switch also sees every relax before it.
        self.switches.lock().expect("switch log").push(on);
        true
    }
    fn relax_layout(&mut self) {
        self.relaxes.fetch_add(1, Ordering::Relaxed);
    }
    fn population(&self) -> u64 {
        0
    }
    fn heap_bytes(&self) -> usize {
        0
    }
}

/// A model author's bug, from the engine's point of view.
struct DivideByZero(u64);

impl SimState for DivideByZero {
    fn step(&mut self) {
        let zero: u64 = std::hint::black_box(0);
        self.0 = 1 / zero;
    }
    fn tick(&self) -> u64 {
        self.0
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

/// A panicking kernel must not take the thread with it and leave the UI polling a viewport
/// that never updates again. The panic still prints. This test is noisy by design.
#[test]
fn a_panicking_step_lands_in_the_sink_instead_of_killing_the_thread() {
    let faults = FaultSink::new();
    let wakes = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&wakes);

    let mut thread = SimThread::new(
        Box::new(DivideByZero(0)),
        1000.0,
        Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        })),
        faults.clone(),
    );
    thread.play();

    assert!(wait_until(|| faults.is_set()), "the panic never reached the sink");
    let fault = faults.take().expect("the panic should have reached the sink");
    assert_eq!(fault.during, STEPPING);
    assert!(fault.to_string().contains("divide by zero"), "{fault}");
    // Without the wake the UI would sit idle and never come and look.
    assert!(wakes.load(Ordering::Relaxed) > 0, "the UI was never woken");
}

#[test]
fn capped_batching_holds_the_target_rate() {
    let (mut thread, ticks, _) = counter(50.0, None);
    thread.send(SimCommand::SetTicksPerSnapshot(10));
    thread.play();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    thread.pause();

    let n = ticks.load(Ordering::Relaxed);
    assert!((20..=150).contains(&n), "ran {n} ticks in 1s at 50 TPS");
}

/// Waits out a window in which a loop that has stopped has to stay still.
///
/// Only a check that nothing happens needs it, and each caller has already seen the loop stop.
fn settle() {
    std::thread::sleep(std::time::Duration::from_millis(100));
}

/// A pause reports a rate of zero, as the GPU runner does, and a step after a long pause is not
/// divided by the whole pause.
#[test]
fn a_pause_and_a_step_after_it_report_no_rate() {
    let (mut thread, _, _) = counter(1000.0, None);

    thread.play();
    // A TPS window is a second wide, and nothing is reported until one closes.
    assert!(
        snapshot_where(&mut thread, |snap| snap.actual_tps > 0.0).is_some(),
        "the window never closed, so the test proves nothing"
    );

    thread.pause();
    // Every snapshot of the running sim since the window closed reports a rate. The first snapshot without a rate
    // is the one the pause published.
    let paused = snapshot_where(&mut thread, |snap| snap.actual_tps == 0.0).expect("a paused sim reported a rate");

    // A single step must not be divided by this stale window.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    thread.step_once();
    let stepped = snapshot_at(&mut thread, paused.tick + 1).expect("a step publishes");
    assert_eq!(stepped.actual_tps, 0.0, "one step over a long pause reported a rate");
}

/// Switching the layout off must reach the state.
/// Otherwise the next publish would still run the layout, and a paused network would keep moving whenever an
/// action or a step publishes.
#[test]
fn switching_the_layout_off_reaches_the_state() {
    let (mut thread, switches, _) = Relaxes::spawn();
    thread.send(SimCommand::SetLayout {
        on: true,
        budget_ms: 1.0,
        while_paused: false,
    });
    thread.send(SimCommand::SetLayout {
        on: false,
        budget_ms: 1.0,
        while_paused: false,
    });
    let log = || switches.lock().expect("switch log").clone();
    assert!(wait_until(|| log().len() >= 2), "only {:?} reached the state", log());
    assert_eq!(log(), [true, false]);
}

/// While paused, the layout moves on a step, and otherwise only when asked to relax while paused.
#[test]
fn a_paused_layout_relaxes_on_a_step_or_when_asked() {
    let (mut thread, switches, relaxes) = Relaxes::spawn();
    let count = || relaxes.load(Ordering::Relaxed);
    let layout = |while_paused| SimCommand::SetLayout {
        on: true,
        budget_ms: 1.0,
        while_paused,
    };
    assert!(
        thread.take_snapshot().is_some(),
        "a new thread publishes its first state"
    );

    thread.send(layout(false));
    assert!(
        snapshot_where(&mut thread, |_| true).is_some(),
        "a layout switch publishes"
    );
    assert_eq!(count(), 0, "a paused publish relaxed");

    thread.step_once();
    assert!(snapshot_at(&mut thread, 1).is_some(), "a step publishes");
    assert_eq!(count(), 1, "a step relaxes once and no more");

    thread.send(layout(true));
    assert!(
        wait_until(|| count() >= 3),
        "relaxing while paused stopped at {}",
        count()
    );

    thread.send(layout(false));
    // The switch reaches the state while the loop handles the command, and no relax runs after it.
    assert!(
        wait_until(|| switches.lock().expect("switch log").len() == 3),
        "the switch never reached the state"
    );
    let stopped = count();
    settle();
    assert_eq!(count(), stopped, "the layout kept relaxing once told to stop");
}

/// A run to a tick ends paused, and a paused layout relaxes after it as it does after Pause.
#[test]
fn a_paused_layout_keeps_relaxing_after_a_run_to() {
    let (mut thread, _, relaxes) = Relaxes::spawn();
    let count = || relaxes.load(Ordering::Relaxed);

    thread.send(SimCommand::SetLayout {
        on: true,
        budget_ms: 1.0,
        while_paused: true,
    });
    thread.run_to(5);
    // The target's snapshot is published after the run has stopped, so every relax from here on is a paused one.
    assert!(
        snapshot_at(&mut thread, 5).is_some(),
        "the run never published its target"
    );
    let reached = count();
    assert!(
        wait_until(|| count() > reached),
        "the layout stopped relaxing at the run's target"
    );
}

/// A snapshot nobody is told about is a snapshot nobody draws. A step has to wake the UI, or the
/// viewport refreshes only once the mouse moves.
#[test]
fn a_publish_while_paused_wakes_the_ui() {
    let wakes = Arc::new(AtomicU64::new(0));
    let woken = Arc::clone(&wakes);
    let (mut thread, _, _) = counter(
        50.0,
        Some(Arc::new(move || {
            woken.fetch_add(1, Ordering::Relaxed);
        })),
    );

    thread.step_once();
    assert!(
        wait_until(|| wakes.load(Ordering::Relaxed) >= 1),
        "a single step published without waking the UI"
    );

    // Pause force-publishes a final snapshot too.
    let before = wakes.load(Ordering::Relaxed);
    thread.pause();
    assert!(
        wait_until(|| wakes.load(Ordering::Relaxed) > before),
        "pausing published a final snapshot without waking the UI"
    );
}

/// Takes snapshots until one satisfies `wanted`, or gives up after the deadline of [`wait_until`].
fn snapshot_where(thread: &mut SimThread, wanted: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
    let mut found = None;
    wait_until(|| {
        found = thread.take_snapshot().filter(|snap| wanted(snap));
        found.is_some()
    });
    found
}

/// Takes snapshots until one reports `tick`, or gives up after the deadline of [`wait_until`].
fn snapshot_at(thread: &mut SimThread, tick: u64) -> Option<Snapshot> {
    snapshot_where(thread, |snap| snap.tick == tick)
}

/// Returns a schedule of one action at each of `ticks`, in the order given.
fn schedule_at(ticks: &[u64]) -> Schedule {
    let entries = ticks
        .iter()
        .map(|&tick| Scheduled {
            index: 0,
            id: "mark".to_owned(),
            tick,
        })
        .collect();
    Schedule::from_entries(entries)
}

fn fired_ticks(fired: &Arc<Mutex<Vec<u64>>>) -> Vec<u64> {
    fired.lock().expect("action log").clone()
}

#[test]
fn run_to_stops_at_the_target_and_pauses() {
    // Capped at 1 TPS, so only an uncapped run reaches the target in time.
    let (mut thread, ticks, _) = counter(1.0, None);
    thread.run_to(20_000);
    let reached = snapshot_at(&mut thread, 20_000).expect("the run never published its target");
    assert_eq!(reached.actual_tps, 0.0, "a paused run reported a rate");
    settle();
    assert_eq!(ticks.load(Ordering::Relaxed), 20_000, "the run went past its target");
    assert!(thread.take_snapshot().is_none(), "a paused run kept publishing");

    thread.run_to(10);
    assert!(
        snapshot_at(&mut thread, 20_000).is_some(),
        "a run to a tick behind the current one pauses and publishes"
    );
    settle();
    assert_eq!(ticks.load(Ordering::Relaxed), 20_000);
}

/// A run to a tick that ends between two publishes still hands the host evenly spaced samples, each holding the
/// stats of its own tick.
#[test]
fn a_run_to_a_tick_samples_its_stats_on_the_way() {
    let (mut thread, _, _) = counter(1.0, None);
    thread.run_to(1000);
    let mut samples = Vec::new();
    let reached = wait_until(|| match thread.take_snapshot() {
        Some(snap) => {
            samples.extend(snap.run_to_samples);
            snap.tick == 1000
        }
        None => false,
    });
    assert!(reached, "the run never published its target");
    let ticks: Vec<u64> = samples.iter().map(|sample| sample.tick()).collect();
    assert_eq!(ticks, (1..500).map(|i| i * 2).collect::<Vec<_>>());
    for sample in &samples {
        assert_eq!(sample.entries()[0].value().scalar(), sample.tick() as f64);
    }
}

#[test]
fn a_schedule_fires_once_per_tick_after_the_step_reaching_it() {
    let (mut thread, ticks, fired) = counter(1000.0, None);
    thread.set_schedule(schedule_at(&[3, 1, 3, 9, 10, 25, 60]));
    thread.run_to(10);
    assert!(snapshot_at(&mut thread, 10).is_some());
    assert_eq!(
        fired_ticks(&fired),
        [1, 3, 3, 9, 10],
        "the step reaching the target fires its actions"
    );

    thread.step_once();
    thread.step_once();
    assert!(snapshot_at(&mut thread, 12).is_some());
    assert_eq!(
        fired_ticks(&fired),
        [1, 3, 3, 9, 10],
        "the target's actions fired twice"
    );

    thread.play();
    assert!(
        wait_until(|| ticks.load(Ordering::Relaxed) >= 30),
        "the sim never played"
    );
    // Ten seconds ahead at 1000 TPS. The run to it stops Play long before the sim gets there.
    let end = ticks.load(Ordering::Relaxed) + 10_000;
    thread.run_to(end);
    assert!(snapshot_at(&mut thread, end).is_some());
    assert_eq!(fired_ticks(&fired), [1, 3, 3, 9, 10, 25, 60]);
}

#[test]
fn set_schedule_fires_what_is_due_now() {
    let (mut thread, ticks, fired) = counter(1000.0, None);
    assert!(
        thread.take_snapshot().is_some(),
        "a new thread publishes its first state"
    );
    thread.set_schedule(schedule_at(&[0, 1, 0]));
    assert!(snapshot_at(&mut thread, 0).is_some(), "setting a schedule publishes");
    assert_eq!(
        fired_ticks(&fired),
        [0, 0],
        "both actions due at tick 0 fire before any step"
    );
    assert_eq!(ticks.load(Ordering::Relaxed), 0);

    thread.step_once();
    assert!(snapshot_at(&mut thread, 1).is_some());
    assert_eq!(fired_ticks(&fired), [0, 0, 1]);
}

/// A second schedule must not fire the actions of the tick the loop sits at. That tick has fired already.
#[test]
fn a_replaced_schedule_never_fires_a_tick_again() {
    let (mut thread, _, fired) = counter(1000.0, None);
    thread.set_schedule(schedule_at(&[0, 4]));
    thread.set_schedule(schedule_at(&[0, 4, 9]));
    thread.run_to(4);
    assert!(snapshot_at(&mut thread, 4).is_some());
    assert_eq!(
        fired_ticks(&fired),
        [0, 4],
        "a second schedule at tick 0 fired it again"
    );

    thread.set_schedule(schedule_at(&[4, 9]));
    thread.run_to(9);
    assert!(snapshot_at(&mut thread, 9).is_some());
    assert_eq!(
        fired_ticks(&fired),
        [0, 4, 9],
        "a schedule at a tick a step reached fired it again"
    );

    // Tick 12 fired nothing, and has had its turn all the same.
    thread.run_to(12);
    assert!(snapshot_at(&mut thread, 12).is_some());
    thread.set_schedule(schedule_at(&[12, 13]));
    thread.step_once();
    assert!(snapshot_at(&mut thread, 13).is_some());
    assert_eq!(fired_ticks(&fired), [0, 4, 9, 13]);
}

#[test]
fn play_cancels_a_run_to() {
    let (mut thread, ticks, _) = counter(20.0, None);
    thread.run_to(u64::MAX);
    // A thousand ticks take 50 s at the cap, so reaching them within the deadline means the run is uncapped.
    assert!(
        wait_until(|| ticks.load(Ordering::Relaxed) > 1000),
        "the run to a tick never ran uncapped"
    );

    thread.play();
    // Once Play resets the TPS window, the next rate measured is the capped one. An uncapped run would report
    // thousands of ticks per second.
    let playing = snapshot_where(&mut thread, |snap| snap.actual_tps > 0.0 && snap.actual_tps <= 60.0);
    thread.pause();
    assert!(playing.is_some(), "Play never brought the sim back to its 20 TPS cap");
}
