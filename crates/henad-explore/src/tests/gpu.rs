//! Checks of GPU sweeps. Each test skips on a machine without a device unless `HENAD_REQUIRE_GPU` is set.

use std::sync::Arc;

use henad_compute::entry::ModelState;
use henad_compute::fault::{Fault, FaultSink, STEPPING};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::{GpuSimState, MAX_STEPS_PER_SUBMISSION, StatsPoll};
use henad_compute::snapshot::GpuSnapshot;
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::outcome::{RunStatus, StopReason};
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_core::explore::stop::StopSpec;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::StatEntry;

use crate::exec::{BatchEnd, Concurrency, Executor, GpuTrackDepth, RunRequest, SweepControl};
use crate::sweep::ExploreError;
use crate::tests::support::{
    Collected, ONE_TRACK, OutputTables, Recorder, ScratchDir, baseline_device, entry, headless_device, planned, sweep,
    sweep_options, sweep_with,
};

#[test]
fn a_gpu_sweep_writes_the_same_files_twice() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = vec![
        ("grid_width".to_owned(), "64".to_owned()),
        ("grid_height".to_owned(), "64".to_owned()),
    ];
    spec.run.steps = 40;
    spec.run.replicates = 2;
    spec.measure.stats_every = 4;
    spec.measure.series_every = 8;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param(
            "infection_rate",
            LevelSpec::Values(vec!["0.2".to_owned(), "0.5".to_owned()]),
        )],
        design_seed: None,
    }];

    let scratch = ScratchDir::new("gpu-twice");
    let tables: Vec<OutputTables> = ["first", "second"]
        .into_iter()
        .map(|name| {
            let output_dir = scratch.path().join(name);
            let report = sweep(&gpu_sir, Some(&ctx), &spec, &output_dir, Concurrency::Auto);
            assert_eq!(
                report.outline.layout.gpu_tracks, 4,
                "one track per run, up to the auto cap"
            );
            assert_eq!((report.counts.rows, report.counts.ok), (4, 4));
            OutputTables::read(&output_dir)
        })
        .collect();
    assert_eq!(
        tables[0].series.len(),
        1 + 4 * 6,
        "ticks 0, 8, 16, 24, 32 and 40 of every run"
    );
    assert_eq!(tables[0], tables[1]);
    let infected = tables[0].run_column("Infected:max");
    assert_ne!(infected[..2], infected[2..], "the infection rate matters");
}

#[test]
fn a_gpu_config_that_does_not_fit_is_refused_before_any_run() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let mut spec = SweepSpec::new("gpu_sir");
    spec.run.steps = 10;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Zip,
        factors: ["grid_width", "grid_height"]
            .map(|id| FactorSpec::param(id, LevelSpec::Values(vec!["64".to_owned(), "6000".to_owned()])))
            .to_vec(),
        design_seed: None,
    }];
    let scratch = ScratchDir::new("gpu-too-big");
    let mut progress = Recorder::default();
    let error = sweep_with(
        &gpu_sir,
        Some(&ctx),
        &spec,
        scratch.path(),
        &sweep_options(false),
        &mut progress,
    )
    .expect_err("a 6000 by 6000 grid passes the baseline's binding size");
    let ExploreError::Capacity(capacity) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(capacity.count, 1);
    assert_eq!(capacity.refused[0].config_id, 1);
    assert!(progress.committed.is_empty());
    assert!(!scratch.path().exists(), "nothing is written before the check");
}

/// Returns a `gpu_sir` sweep of 2 replicates on a 32 by 32 grid, keeping every sample in the series.
fn gpu_sir_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = [
        ("grid_width", "32"),
        ("grid_height", "32"),
        ("recovery_rate", "0.2"),
        ("initial_infected_pct", "0.2"),
    ]
    .map(|(id, value)| (id.to_owned(), value.to_owned()))
    .to_vec();
    spec.run.steps = 60;
    spec.run.replicates = 2;
    spec.measure.stats_every = 4;
    spec.measure.series_every = 4;
    spec.seeds.root = 6;
    spec
}

#[test]
fn a_gpu_run_stops_on_the_first_sample_where_the_condition_holds() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let scratch = ScratchDir::new("gpu-stop");
    let spec = gpu_sir_spec();
    sweep(
        &gpu_sir,
        Some(&ctx),
        &spec,
        &scratch.path().join("full"),
        Concurrency::Auto,
    );
    let full = OutputTables::read(&scratch.path().join("full"));
    let threshold = full
        .series_of(0, "Recovered")
        .into_iter()
        .find(|&(tick, _)| tick == 16)
        .map(|(_, value)| value.to_owned())
        .expect("run 0 samples Recovered at tick 16");

    let mut stopping = spec.clone();
    stopping.run.stop = Some(StopSpec::parse(&format!("Recovered >= {threshold}"), 0).expect("a valid condition"));
    sweep(
        &gpu_sir,
        Some(&ctx),
        &stopping,
        &scratch.path().join("stopped"),
        Concurrency::Auto,
    );
    let stopped = OutputTables::read(&scratch.path().join("stopped"));
    let threshold: f64 = threshold.parse().expect("a number");
    for run_id in 0..2 {
        let full_series = full.series_of(run_id, "Recovered");
        let expected = full_series
            .iter()
            .find(|&&(_, value)| value.parse::<f64>().is_ok_and(|value| value >= threshold))
            .map(|&(tick, _)| tick)
            .expect("Recovered never falls");
        let row = run_id as usize;
        assert_eq!(stopped.run_column("ticks")[row], expected.to_string(), "run {run_id}");
        assert_eq!(stopped.run_column("stop_reason")[row], "condition");
        let kept: Vec<(u64, &str)> = full_series.into_iter().filter(|&(tick, _)| tick <= expected).collect();
        assert_eq!(stopped.series_of(run_id, "Recovered"), kept);
    }
}

#[test]
fn a_gpu_action_tick_factor_moves_the_action() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let mut spec = gpu_sir_spec();
    spec.run.replicates = 1;
    spec.fixed.push(("infection_rate".to_owned(), "0.1".to_owned()));
    spec.actions = vec![ActionSpec {
        name: "wave".to_owned(),
        ..ActionSpec::new("seed_outbreak", 0)
    }];
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::action(
            "wave",
            LevelSpec::Values(vec!["0".to_owned(), "12".to_owned(), "40".to_owned()]),
        )],
        design_seed: None,
    }];
    let scratch = ScratchDir::new("gpu-action-tick");
    sweep(&gpu_sir, Some(&ctx), &spec, scratch.path(), Concurrency::Auto);
    let tables = OutputTables::read(scratch.path());
    assert_eq!(tables.run_column("action.wave"), ["0", "12", "40"]);
    assert_eq!(tables.run_column("status"), ["ok", "ok", "ok"]);
    let [at_start, at_twelve, at_forty] = [0, 1, 2].map(|run_id| tables.series_of(run_id, "Infected"));
    assert_ne!(
        at_start[0], at_twelve[0],
        "an action at tick 0 fires before the first sample"
    );
    assert_eq!(
        at_twelve[..3],
        at_forty[..3],
        "ticks 0, 4 and 8 come before either action"
    );
    assert_eq!(at_twelve[3].0, 12);
    assert_ne!(
        at_twelve[3], at_forty[3],
        "the sample at tick 12 sees the action due there"
    );
}

/// GPU state that raises a device error in the batch of steps reaching tick `fault_tick`, as a failing step pass
/// would.
struct FaultsAtTick {
    state: Box<dyn GpuSimState>,
    faults: FaultSink,
    fault_tick: u64,
}

impl SimState for FaultsAtTick {
    fn step(&mut self) {
        self.state.step();
    }

    fn tick(&self) -> u64 {
        self.state.tick()
    }

    fn stats(&self) -> Vec<StatEntry> {
        self.state.stats()
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.state.set_param(index, value)
    }

    fn population(&self) -> u64 {
        self.state.population()
    }

    fn heap_bytes(&self) -> usize {
        self.state.heap_bytes()
    }
}

impl GpuSimState for FaultsAtTick {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        let start = self.state.tick();
        self.state.encode_steps(encoder, count, timestamps);
        if start < self.fault_tick && self.fault_tick <= self.state.tick() {
            let error = wgpu::Error::Validation {
                source: "the step pass failed".into(),
                description: "the step pass failed".to_owned(),
            };
            self.faults.set_once(Fault::device(STEPPING, error));
        }
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.state.encode_snapshot_passes(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.state.begin_stats_readback();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        self.state.poll_stats_readback(device, block)
    }

    fn stats_readback_pending(&self) -> bool {
        self.state.stats_readback_pending()
    }

    fn view(&self) -> GpuSnapshot {
        self.state.view()
    }
}

#[test]
fn a_gpu_fault_while_stepping_records_the_same_outcome_at_any_slice_size() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = vec![
        ("grid_width".to_owned(), "32".to_owned()),
        ("grid_height".to_owned(), "32".to_owned()),
    ];
    spec.run.warmup = 10;
    spec.run.steps = 20;
    spec.measure.stats_every = 4;
    spec.measure.series_every = 4;
    let (plan, measure) = planned(&gpu_sir, Some(&ctx), &spec);

    let faults = ctx.faults.clone();
    let faulting = gpu_sir.wrap_factory(|create| {
        Arc::new(
            move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| match create(params, seed, gpu)? {
                ModelState::Gpu(state) => Ok(ModelState::Gpu(Box::new(FaultsAtTick {
                    state,
                    faults: faults.clone(),
                    fault_tick: 17,
                }))),
                ModelState::Cpu(state) => Ok(ModelState::Cpu(state)),
            },
        )
    });
    let outcomes: Vec<_> = [(1, 1), (7, 2), (MAX_STEPS_PER_SUBMISSION, 2)]
        .into_iter()
        .map(|(steps_per_submission, submissions_per_track)| {
            let executor = Executor::new(
                &faulting,
                Some(&ctx),
                Arc::clone(&measure),
                ONE_TRACK,
                SweepControl::new(),
            )
            .expect("a device")
            .with_track_depth(GpuTrackDepth {
                submissions_per_track,
                steps_per_submission,
            });
            let requests: Vec<RunRequest<'_>> = plan.runs().map(|run| RunRequest::planned(&plan, run)).collect();
            let mut collected = Collected::default();
            let end = executor.run_batch(&requests, &mut collected).expect("the batch runs");
            assert_eq!(end, BatchEnd::Complete);
            collected.0.remove(0)
        })
        .collect();
    for outcome in &outcomes {
        assert_eq!(outcome.status, RunStatus::GpuError, "{:?}", outcome.note);
        assert_eq!(outcome.stop_reason, StopReason::Fault);
        assert_eq!(outcome.ticks, 14, "the last sample before the fault at tick 17");
        assert_eq!(outcome.series.ticks(), [10, 14]);
        assert_eq!(
            (&outcome.reducers, &outcome.note),
            (&outcomes[0].reducers, &outcomes[0].note)
        );
    }
}

/// Checks that sampling a GPU run every tick or every tenth tick leaves it on the same trajectory.
///
/// A sample encodes the stats passes between two batches of steps. Each case runs one model at each cadence and
/// compares the rows at the ticks both sample. It also rebuilds the run, samples it along the same cadence and
/// compares the view read back at the end. A model that declares it does not replay exactly, as `gpu_boids` does,
/// has no case.
mod sampling_cadence_does_not_change_the_trajectory {
    use henad_compute::entry::ModelState;
    use henad_compute::gpu::view::display::GpuDisplay;
    use henad_compute::gpu::{GpuContext, GpuSimState, stepping};
    use henad_core::explore::spec::SweepSpec;
    use henad_core::metadata::Backend;
    use henad_models::example_models;

    use crate::tests::support::{Collected, ONE_TRACK, entry, headless_device, planned, run_plan};

    const STEPS: u64 = 25;
    const CADENCES: [u64; 2] = [1, 10];

    /// Registered GPU models that replay exactly, one case each.
    const CASES: [&str; 3] = ["gpu_sir", "gpu_game_of_life", "gpu_ants"];

    /// Rows sampled at the ticks every cadence samples, and the view of the final state.
    #[derive(Debug, PartialEq, Eq)]
    struct Trajectory {
        /// Tick and value bits of each row.
        rows: Vec<(u64, Vec<u64>)>,
        /// Display drawn at the size of its grid, then the position and colour lanes.
        view: Vec<u8>,
    }

    fn trajectory(ctx: &GpuContext, id: &str, fixed_values: &[(&str, &str)], stats_every: u64) -> Trajectory {
        let model = entry(id, Some(ctx));
        let mut spec = SweepSpec::new(id);
        spec.fixed = fixed_values
            .iter()
            .map(|&(param, value)| (param.to_owned(), value.to_owned()))
            .collect();
        spec.run.steps = STEPS;
        spec.measure.stats_every = stats_every;
        spec.measure.series_every = stats_every;
        spec.seeds.root = 5;
        let (plan, measure) = planned(&model, Some(ctx), &spec);
        let mut collected = Collected::default();
        run_plan(&model, Some(ctx), &plan, &measure, ONE_TRACK, &mut collected);
        let outcome = &collected.0[0];
        assert_eq!(outcome.ticks, STEPS, "{:?}", outcome.note);
        let rows = outcome
            .series
            .rows()
            .filter(|&(tick, _)| tick == STEPS || tick % CADENCES[1] == 0)
            .map(|(tick, values)| (tick, values.iter().map(|value| value.to_bits()).collect()))
            .collect();

        let run = plan.run(0).expect("the plan has a run");
        let params = &plan.config(run.config_id).expect("the run's config").params;
        let Ok(ModelState::Gpu(mut state)) = model.build(params, Some(run.seed), Some(ctx)) else {
            panic!("{id} builds on the GPU");
        };
        for tick in measure.sample_ticks() {
            let steps = tick - state.tick();
            stepping::run_steps(&mut *state, ctx, steps).expect("the steps run");
            assert!(
                !stepping::sample_stats(&mut *state, ctx)
                    .expect("the sample lands")
                    .is_empty()
            );
        }
        // A sample records the stats passes alone, and the display pass of a snapshot draws the view read below.
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_test_display"),
        });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        Trajectory {
            rows,
            view: view_bytes(&*state, ctx),
        }
    }

    /// Returns the view of `state` as bytes, read back from the device.
    fn view_bytes(state: &dyn GpuSimState, ctx: &GpuContext) -> Vec<u8> {
        let view = state.view();
        let mut bytes = Vec::new();
        if let Some(display) = &view.display {
            bytes.extend(drawn(display, ctx));
        }
        if let Some(agents) = &view.agents {
            bytes.extend(copied(&agents.pos, ctx));
            bytes.extend(copied(&agents.color, ctx));
        }
        assert!(!bytes.is_empty(), "the model has a view to read");
        bytes
    }

    /// Returns the pixels of `display` drawn into a texture the size of its grid, row after row.
    fn drawn(display: &GpuDisplay, ctx: &GpuContext) -> Vec<u8> {
        let size = wgpu::Extent3d {
            width: display.width,
            height: display.height,
            depth_or_array_layers: 1,
        };
        let target = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("henad_test_display_target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ctx.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let row_bytes = display.width * 4;
        let padded_row = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_test_display_staging"),
            size: u64::from(padded_row) * u64::from(display.height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_test_display"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("henad_test_display_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&display.render_pipeline);
            pass.set_bind_group(0, &display.render_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(display.height),
                },
            },
            size,
        );
        ctx.queue.submit(Some(encoder.finish()));
        let pixels: Vec<u8> = mapped(&staging, ctx)
            .chunks(padded_row as usize)
            .flat_map(|row| row[..row_bytes as usize].to_vec())
            .collect();
        assert!(pixels.iter().any(|&byte| byte != 0), "the display was drawn");
        pixels
    }

    /// Returns the bytes of `buffer`, copied through a staging buffer.
    fn copied(buffer: &wgpu::Buffer, ctx: &GpuContext) -> Vec<u8> {
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_test_lane_staging"),
            size: buffer.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_test_lane_copy"),
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
        ctx.queue.submit(Some(encoder.finish()));
        mapped(&staging, ctx)
    }

    /// Returns the bytes of `staging` once the device has written them.
    fn mapped(staging: &wgpu::Buffer, ctx: &GpuContext) -> Vec<u8> {
        let (sender, receiver) = std::sync::mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| drop(sender.send(result)));
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device finishes its work");
        receiver
            .recv()
            .expect("the map reports back")
            .expect("the staging buffer maps");
        let bytes = staging
            .slice(..)
            .get_mapped_range()
            .expect("the mapped range reads")
            .to_vec();
        staging.unmap();
        bytes
    }

    fn check(id: &str, fixed_values: &[(&str, &str)]) {
        let Some(ctx) = headless_device() else {
            return;
        };
        let [every_tick, every_tenth] = CADENCES.map(|stats_every| trajectory(&ctx, id, fixed_values, stats_every));
        assert_eq!(every_tick.rows.len(), 4, "ticks 0, 10, 20 and {STEPS}");
        assert_eq!(every_tick, every_tenth, "{id}");
    }

    #[test]
    fn every_gpu_model_that_replays_has_a_case() {
        let mut registered: Vec<String> = example_models()
            .iter()
            .filter(|model| model.metadata().backend == Backend::Gpu && model.metadata().replays_exactly)
            .map(|model| model.id().to_owned())
            .collect();
        registered.sort_unstable();
        let mut covered: Vec<String> = CASES.iter().map(|&id| id.to_owned()).collect();
        covered.sort_unstable();
        assert_eq!(covered, registered);
    }

    #[test]
    fn gpu_sir() {
        check("gpu_sir", &[("grid_width", "32"), ("grid_height", "32")]);
    }

    #[test]
    fn gpu_game_of_life() {
        check("gpu_game_of_life", &[("grid_width", "32"), ("grid_height", "32")]);
    }

    #[test]
    fn gpu_ants() {
        check(
            "gpu_ants",
            &[("num_agents", "1000"), ("world_width", "64"), ("world_height", "64")],
        );
    }
}

/// Checks that a sweep records the adapter of the context it is handed for a GPU model, and none for a CPU model
/// handed the same context, which steps on no device.
#[test]
fn a_sweep_records_the_adapter_of_the_context_it_steps_on() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let adapter = ctx
        .runtime_info()
        .expect("a headless device carries its runtime info")
        .adapter
        .name
        .clone();
    let scratch = ScratchDir::new("sweep-adapter");
    let gpu_dir = scratch.path().join("gpu");
    sweep(
        &entry("gpu_sir", Some(&ctx)),
        Some(&ctx),
        &gpu_sir_spec(),
        &gpu_dir,
        Concurrency::Auto,
    );
    let recorded = crate::tests::support::manifest(&gpu_dir).runtime.adapter;
    assert_eq!(recorded, Some(adapter));

    let mut cpu_spec = SweepSpec::new("sir");
    cpu_spec.fixed = vec![
        ("grid_width".to_owned(), "16".to_owned()),
        ("grid_height".to_owned(), "16".to_owned()),
    ];
    cpu_spec.run.steps = 4;
    let cpu_dir = scratch.path().join("cpu");
    sweep(&entry("sir", None), Some(&ctx), &cpu_spec, &cpu_dir, Concurrency::Auto);
    assert!(crate::tests::support::manifest(&cpu_dir).runtime.adapter.is_none());
}
