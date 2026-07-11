//! Dedicated OS thread that owns the GPU Game of Life state buffers and steps them flat-out,
//! decoupled from the UI frame rate — mirrors `henad_compute::sim_thread`'s CPU sim thread.
//!
//! # Synchronization
//!
//! `Device` and `Queue` are cheap, `Send + Sync` handles cloned from egui's own render state, so
//! this thread submits work on the same queue egui uses to render. wgpu serializes all
//! submissions to a given queue in the order `submit()` is called, and each submission is
//! atomic from the GPU's point of view (a later submission never observes a partially-executed
//! earlier one). So when this thread's display-texture write and egui's render-pass sample land
//! in different submissions, the render pass either sees the fully-written previous texture or
//! the fully-written next one — never a torn one. The one accepted cost is up to one frame of
//! staleness (the UI may sample last snapshot's texture instead of the newest one), which is
//! fine for a sim running orders of magnitude faster than the display refresh rate.
//!
//! # Batching
//!
//! Steps are batched `batch_size`-per-submission to keep submission overhead from competing with
//! egui's own per-frame submissions on the shared queue. Each step is still its own compute pass
//! (see `GpuGolCompute::dispatch_step_batch` for why — wgpu only synchronizes between passes, not
//! between dispatches within one pass, and the ping-pong buffers need that). The display compute
//! pass (state → texture) only runs when at least ~16ms have elapsed since the last one, so
//! "steps per snapshot" is emergent from how fast the batches run, exactly like the CPU sim
//! thread's `ticks_per_snapshot`.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{GpuGolCompute, ReseedKind, seed_patterns, seed_random};

/// Default steps-per-submission, tunable at runtime from the UI.
pub const DEFAULT_BATCH_SIZE: u32 = 64;

const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(16);
const STATS_INTERVAL: Duration = Duration::from_secs(1);

/// Commands sent from the UI thread to the GPU sim thread.
enum GpuGolCommand {
    Pause,
    Resume,
    SetBatchSize(u32),
    Reseed(ReseedKind),
    Shutdown,
}

/// Latest wall-clock and GPU-side performance numbers, polled by the UI once per frame.
#[derive(Clone, Copy)]
pub struct GpuGolStats {
    pub wall_tps: f64,
    pub gpu_us_per_step: Option<f64>,
    pub batch_size: u32,
    pub paused: bool,
}

impl Default for GpuGolStats {
    fn default() -> Self {
        Self {
            wall_tps: 0.0,
            gpu_us_per_step: None,
            batch_size: DEFAULT_BATCH_SIZE,
            paused: false,
        }
    }
}

/// GPU timestamp-query resources, created only if the device supports `Features::TIMESTAMP_QUERY`.
struct TimestampQuery {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback_buffer: wgpu::Buffer,
    /// Nanoseconds per timestamp tick, from `Queue::get_timestamp_period`.
    period_ns: f32,
}

impl TimestampQuery {
    const BUFFER_SIZE: u64 = 2 * std::mem::size_of::<u64>() as u64;

    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu_gol_timestamp_query_set"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_gol_timestamp_resolve"),
            size: Self::BUFFER_SIZE,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_gol_timestamp_readback"),
            size: Self::BUFFER_SIZE,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            query_set,
            resolve_buffer,
            readback_buffer,
            period_ns: queue.get_timestamp_period(),
        })
    }

    /// Blocking readback of the two timestamps written by the last stamped batch, called at most
    /// once per `STATS_INTERVAL` — the stall this introduces is negligible next to a sim running
    /// at thousands of TPS.
    fn read_gpu_us_per_step(&self, device: &wgpu::Device, batch_size: u32) -> Option<f64> {
        let slice = self.readback_buffer.slice(..);
        let (tx, rx) = flume::bounded(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            drop(tx.send(result));
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let data = slice.get_mapped_range();
        let ticks: &[u64] = bytemuck::cast_slice(&data);
        let (start, end) = (ticks[0], ticks[1]);
        drop(data);
        self.readback_buffer.unmap();

        let elapsed_ns = end.saturating_sub(start) as f64 * f64::from(self.period_ns);
        Some(elapsed_ns / 1000.0 / f64::from(batch_size.max(1)))
    }
}

struct GpuGolSimLoop {
    device: wgpu::Device,
    queue: wgpu::Queue,
    compute: GpuGolCompute,
    cmd_rx: mpsc::Receiver<GpuGolCommand>,
    stats: Arc<Mutex<GpuGolStats>>,
    running: bool,
    batch_size: u32,
    step_count: u64,
    tps_timer: Instant,
    last_display_publish: Instant,
    last_stats_publish: Instant,
    timestamp_query: Option<TimestampQuery>,
}

impl GpuGolSimLoop {
    fn run(mut self) {
        loop {
            if !self.running {
                let Ok(cmd) = self.cmd_rx.recv() else {
                    return;
                };
                if self.handle_command(cmd) {
                    return;
                }
                continue;
            }

            self.step_batch();

            while let Ok(cmd) = self.cmd_rx.try_recv() {
                if self.handle_command(cmd) {
                    return;
                }
            }
        }
    }

    /// Returns true if the thread should exit.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "consumes the command to extract owned data (e.g. ReseedKind), matching henad_compute::sim_thread's SimCommand handling"
    )]
    fn handle_command(&mut self, cmd: GpuGolCommand) -> bool {
        match cmd {
            GpuGolCommand::Pause => {
                self.running = false;
                self.publish_stats(0.0, None);
            }
            GpuGolCommand::Resume => {
                self.running = true;
                self.tps_timer = Instant::now();
                self.step_count = 0;
            }
            GpuGolCommand::SetBatchSize(n) => {
                self.batch_size = n.max(1);
            }
            GpuGolCommand::Reseed(kind) => {
                let cells = match kind {
                    ReseedKind::Patterns => seed_patterns(self.compute.width, self.compute.height),
                    ReseedKind::Random { seed, density } => {
                        seed_random(self.compute.width, self.compute.height, density, seed)
                    }
                };
                self.compute.reseed(&self.queue, &cells);
                // Refresh the display texture immediately, even if paused, so reseeding shows
                // the new grid right away instead of waiting for the next running step_batch.
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("gpu_gol_reseed_display_encoder"),
                        });
                self.compute.dispatch_display(&mut encoder);
                self.queue.submit(Some(encoder.finish()));
                self.last_display_publish = Instant::now();
            }
            GpuGolCommand::Shutdown => return true,
        }
        false
    }

    /// Records and submits one batch of steps (plus, at snapshot/stats cadence, the display pass
    /// and/or a timestamped-query resolve), then updates the published stats when due.
    fn step_batch(&mut self) {
        let now = Instant::now();
        let want_timing = self.timestamp_query.is_some()
            && now.duration_since(self.last_stats_publish) >= STATS_INTERVAL;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gpu_gol_sim_encoder"),
            });

        let query_set = if want_timing {
            self.timestamp_query.as_ref().map(|tq| &tq.query_set)
        } else {
            None
        };
        self.compute
            .dispatch_step_batch(&mut encoder, self.batch_size, query_set);

        if now.duration_since(self.last_display_publish) >= SNAPSHOT_INTERVAL {
            self.compute.dispatch_display(&mut encoder);
            self.last_display_publish = now;
        }

        if want_timing {
            let tq = self
                .timestamp_query
                .as_ref()
                .expect("want_timing implies timestamp_query is Some");
            encoder.resolve_query_set(&tq.query_set, 0..2, &tq.resolve_buffer, 0);
            encoder.copy_buffer_to_buffer(
                &tq.resolve_buffer,
                0,
                &tq.readback_buffer,
                0,
                TimestampQuery::BUFFER_SIZE,
            );
        }

        self.queue.submit(Some(encoder.finish()));
        self.step_count += u64::from(self.batch_size);

        if want_timing {
            let gpu_us_per_step = self
                .timestamp_query
                .as_ref()
                .expect("want_timing implies timestamp_query is Some")
                .read_gpu_us_per_step(&self.device, self.batch_size);
            let wall_tps = self.step_count as f64
                / now
                    .duration_since(self.tps_timer)
                    .as_secs_f64()
                    .max(f64::EPSILON);
            self.step_count = 0;
            self.tps_timer = now;
            self.last_stats_publish = now;
            self.publish_stats(wall_tps, gpu_us_per_step);
        } else if self.timestamp_query.is_none()
            && now.duration_since(self.tps_timer) >= STATS_INTERVAL
        {
            // No GPU timing support on this device/backend — still publish wall-clock TPS on
            // the same cadence so the UI keeps updating.
            let wall_tps = self.step_count as f64
                / now
                    .duration_since(self.tps_timer)
                    .as_secs_f64()
                    .max(f64::EPSILON);
            self.step_count = 0;
            self.tps_timer = now;
            self.publish_stats(wall_tps, None);
        }
    }

    fn publish_stats(&self, wall_tps: f64, gpu_us_per_step: Option<f64>) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.wall_tps = wall_tps;
            stats.gpu_us_per_step = gpu_us_per_step;
            stats.batch_size = self.batch_size;
            stats.paused = !self.running;
        }
    }
}

/// Handle to the GPU sim thread: send commands, poll the latest stats. Dropping it shuts the
/// thread down and joins it (mirrors `henad_compute::sim_thread::SimThread`'s `Drop`).
pub struct GpuGolHandle {
    cmd_tx: mpsc::Sender<GpuGolCommand>,
    stats: Arc<Mutex<GpuGolStats>>,
    handle: Option<JoinHandle<()>>,
}

impl GpuGolHandle {
    /// Spawns the GPU sim thread, taking ownership of `compute` and cloned `device`/`queue`
    /// handles. Starts running immediately (unpaused).
    pub fn spawn(
        device: wgpu::Device,
        queue: wgpu::Queue,
        compute: GpuGolCompute,
        initial_batch_size: u32,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let batch_size = initial_batch_size.max(1);
        let stats = Arc::new(Mutex::new(GpuGolStats {
            batch_size,
            ..GpuGolStats::default()
        }));
        let stats_clone = Arc::clone(&stats);
        let timestamp_query = TimestampQuery::new(&device, &queue);

        let sim_loop = GpuGolSimLoop {
            device,
            queue,
            compute,
            cmd_rx,
            stats: stats_clone,
            running: true,
            batch_size,
            step_count: 0,
            tps_timer: Instant::now(),
            last_display_publish: Instant::now(),
            last_stats_publish: Instant::now(),
            timestamp_query,
        };

        let handle = std::thread::spawn(move || sim_loop.run());

        Self {
            cmd_tx,
            stats,
            handle: Some(handle),
        }
    }

    pub fn pause(&self) {
        self.send(GpuGolCommand::Pause);
    }

    pub fn resume(&self) {
        self.send(GpuGolCommand::Resume);
    }

    pub fn set_batch_size(&self, batch_size: u32) {
        self.send(GpuGolCommand::SetBatchSize(batch_size));
    }

    pub(crate) fn reseed(&self, kind: ReseedKind) {
        self.send(GpuGolCommand::Reseed(kind));
    }

    /// Latest published stats. Cheap: a single mutex lock and copy.
    pub fn stats(&self) -> GpuGolStats {
        self.stats
            .lock()
            .map_or_else(|_| GpuGolStats::default(), |s| *s)
    }

    fn send(&self, cmd: GpuGolCommand) {
        drop(self.cmd_tx.send(cmd));
    }
}

impl Drop for GpuGolHandle {
    fn drop(&mut self) {
        drop(self.cmd_tx.send(GpuGolCommand::Shutdown));
        if let Some(h) = self.handle.take() {
            drop(h.join());
        }
    }
}
