//! The example models as a list, for the hosts that still index one.

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::metadata::Backend;

/// Every example model, GPU ones only when `gpu` holds a device.
///
/// Without a device the GPU models are left out, so a host lists only models it can run.
#[expect(clippy::needless_pass_by_value)]
pub fn model_registry(gpu: Option<GpuContext>) -> Vec<ModelEntry> {
    crate::example_models()
        .iter()
        .filter(|entry| gpu.is_some() || entry.metadata().backend != Backend::Gpu)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use henad_compute::entry::{ModelState, register_grid_model};
    use henad_compute::fault::{BUILDING, catching};
    use henad_compute::gpu::{MAX_STEPS_PER_SUBMISSION, StatsPoll, stepping};
    use henad_core::metadata::Structure;
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_core::topology::TopologyHint;

    use super::*;

    /// A baseline device, or `None` when this machine cannot give one.
    ///
    /// The device asks for `Limits::default()`, so a GPU model that only fits a raised limit fails
    /// to build here. That is deliberate: every model is meant to run on a stock WebGPU device.
    fn device() -> Option<GpuContext> {
        crate::tests::support::headless_context("registry_test_device", wgpu::Features::empty())
    }

    /// Every entry, GPU ones included when `gpu` holds a device.
    fn all_entries(gpu: Option<&GpuContext>) -> Vec<ModelEntry> {
        model_registry(gpu.cloned())
    }

    fn is_gpu(entry: &ModelEntry) -> bool {
        entry.metadata().backend == Backend::Gpu
    }

    fn defaults(entry: &ModelEntry) -> Vec<ParamValue> {
        entry
            .param_descriptors()
            .iter()
            .map(|desc| desc.kind.default_value())
            .collect()
    }

    /// A model that cannot build from its own defaults has already failed. The tests below treat
    /// a `Fault` as a failure rather than threading it through.
    fn build(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&GpuContext>) -> ModelState {
        entry
            .build(values, None, gpu)
            .unwrap_or_else(|fault| panic!("{}: {fault}", entry.id()))
    }

    /// Both arms are a `SimState`, which is where the contracts below live.
    fn sim_state(state: &mut ModelState) -> &mut dyn SimState {
        match state {
            ModelState::Cpu(state) => state.as_mut(),
            ModelState::Gpu(state) => state.as_mut(),
        }
    }

    /// The UI labels parameters from the descriptor and the state decides what it accepts, so the
    /// two disagreeing means the panel lies about what an edit does.
    #[test]
    fn declared_apply_mode_matches_what_the_state_accepts() {
        let gpu = device();
        for entry in all_entries(gpu.as_ref()) {
            let values = defaults(&entry);
            let mut created = build(&entry, &values, gpu.as_ref());
            let state = sim_state(&mut created);

            for (i, desc) in entry.param_descriptors().iter().enumerate() {
                assert_eq!(
                    state.set_param(i, &values[i]),
                    desc.is_live(),
                    "{}: parameter '{}' is declared {:?} but set_param disagrees",
                    entry.id(),
                    desc.id,
                    desc.apply
                );
            }
        }
    }

    /// Nothing else reads `topology_hint`, so without this it drifts from what the state returns.
    #[test]
    fn declared_topology_matches_the_views_the_state_returns() {
        for entry in model_registry(None) {
            let values = defaults(&entry);
            let ModelState::Cpu(state) = build(&entry, &values, None) else {
                continue;
            };

            assert_eq!(
                state.grid_view().is_some(),
                entry.topology_hint().grid,
                "{}: declares grid={} but grid_view() disagrees",
                entry.id(),
                entry.topology_hint().grid
            );
            assert_eq!(
                state.point_view().is_some(),
                entry.topology_hint().agents,
                "{}: declares agents={} but point_view() disagrees",
                entry.id(),
                entry.topology_hint().agents
            );
            assert_eq!(
                state.edge_view().is_some(),
                entry.topology_hint().edges,
                "{}: declares edges={} but edge_view() disagrees",
                entry.id(),
                entry.topology_hint().edges
            );
        }
    }

    /// The benchmark CSV carries the job count beside the thread count, where a blank cell has to
    /// mean a GPU model rather than a CPU one that reports nothing.
    #[test]
    fn only_a_cpu_model_reports_how_far_a_step_splits() {
        let gpu = device();
        for entry in all_entries(gpu.as_ref()) {
            let values = defaults(&entry);
            let mut created = build(&entry, &values, gpu.as_ref());
            let cpu = matches!(created, ModelState::Cpu(_));
            let jobs = sim_state(&mut created).parallel_jobs();

            assert_eq!(jobs.is_some(), cpu, "{}: reports {jobs:?}", entry.id());
            assert!(jobs.is_none_or(|n| n > 0), "{}: a step splits into no jobs", entry.id());
        }
    }

    /// Nothing but the Model panel reads the metadata, so a mis-registered entry would show the
    /// wrong backend for a whole release without anything else noticing.
    #[test]
    fn declared_metadata_matches_the_entry_it_describes() {
        let device = device();
        for entry in all_entries(device.as_ref()) {
            let (backend, structure) = (entry.metadata().backend, &entry.metadata().structure);
            let gpu = matches!(backend, Backend::Gpu);

            assert_eq!(
                gpu,
                entry.demand(&defaults(&entry), &wgpu::Limits::default()).is_some(),
                "{}: declares {backend:?} but only a GPU entry carries a capacity",
                entry.id()
            );
            assert_eq!(
                gpu,
                entry.gpu_needs().is_some(),
                "{}: declares {backend:?} but only a GPU entry declares device needs",
                entry.id()
            );
            assert_eq!(
                gpu,
                matches!(build(&entry, &defaults(&entry), device.as_ref()), ModelState::Gpu(_)),
                "{}: declares {backend:?} but its factory returns the other arm",
                entry.id()
            );
            assert!(
                gpu || entry.metadata().replays_exactly,
                "{}: a CPU model replays exactly",
                entry.id()
            );

            let hint = entry.topology_hint();
            let agrees = match structure {
                Structure::Grid { .. } | Structure::GpuGrid { .. } => hint == TopologyHint::GRID,
                Structure::Agents { .. } | Structure::GpuAgents { .. } => hint.agents,
                Structure::Network { .. } => hint.agents && hint.edges,
            };
            assert!(agrees, "{}: declared structure and topology disagree", entry.id());

            assert!(
                gpu == matches!(structure, Structure::GpuGrid { .. } | Structure::GpuAgents { .. }),
                "{}: declares {backend:?} but a structure for the other backend",
                entry.id()
            );
        }
    }

    /// The panel draws a button per declared action and the state decides what it runs, so the
    /// two disagreeing means a button that quietly does nothing.
    #[test]
    fn every_declared_action_is_accepted_by_the_state() {
        let gpu = device();
        for entry in all_entries(gpu.as_ref()) {
            let values = defaults(&entry);
            let mut created = build(&entry, &values, gpu.as_ref());
            let declared = entry.action_descriptors().len();
            let state = sim_state(&mut created);

            for (i, action) in entry.action_descriptors().iter().enumerate() {
                assert!(
                    state.act(i),
                    "{}: declares action '{}' at index {i} but the state refuses it",
                    entry.id(),
                    action.id
                );
            }
            assert!(
                !state.act(declared),
                "{}: accepts an action past the {declared} it declares",
                entry.id()
            );
        }
    }

    /// Ids reach the CLI through `--act`, where two the same would be ambiguous.
    #[test]
    fn action_ids_are_unique_within_a_model() {
        for entry in crate::example_models().iter() {
            let mut ids: Vec<&str> = entry.action_descriptors().iter().map(|a| a.id).collect();
            let declared = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), declared, "{}: declares the same action id twice", entry.id());
        }
    }

    /// Every palette is drawn from, so an empty one would colour a model's cells out of an empty
    /// slice.
    #[test]
    fn a_declared_palette_has_colours_in_it() {
        for entry in crate::example_models().iter() {
            let Some(palette) = entry.metadata().palette else {
                continue;
            };
            assert!(!palette.is_empty(), "{}: declares an empty palette", entry.id());
        }
    }

    /// Labels and colours are declared once and paired with values positionally, so a model that
    /// returns too few values loses its trailing series rather than mislabelling anything. Silent
    /// either way, hence this.
    #[test]
    fn every_declared_stat_series_gets_a_value() {
        let gpu = device();
        for entry in all_entries(gpu.as_ref()) {
            let values = defaults(&entry);
            let mut created = build(&entry, &values, gpu.as_ref());
            let state = sim_state(&mut created);
            assert_eq!(
                state.stats().len(),
                entry.stat_descriptors().len(),
                "{}: declares {} stat series but produced {} values",
                entry.id(),
                entry.stat_descriptors().len(),
                state.stats().len()
            );
        }
    }

    /// The GPU counterpart of the test above. A GPU state publishes through its snapshot rather
    /// than through `grid_view`/`point_view`, so that is what the hint has to agree with.
    #[test]
    fn declared_topology_matches_the_layers_a_gpu_state_publishes() {
        let gpu = device();
        for entry in all_entries(gpu.as_ref()) {
            let values = defaults(&entry);
            let ModelState::Gpu(state) = build(&entry, &values, gpu.as_ref()) else {
                continue;
            };
            let view = state.view();
            assert_eq!(
                view.display.is_some(),
                entry.topology_hint().grid,
                "{}: declares grid={} but its snapshot disagrees",
                entry.id(),
                entry.topology_hint().grid
            );
            assert_eq!(
                view.agents.is_some(),
                entry.topology_hint().agents,
                "{}: declares agents={} but its snapshot disagrees",
                entry.id(),
                entry.topology_hint().agents
            );
        }
    }

    /// A model author can get a kernel wrong, and that must reach Build as a message rather than
    /// as a dead process. The panic still prints. This test is noisy by design.
    #[test]
    fn a_model_that_panics_while_building_comes_back_as_a_fault() {
        let entry = register_grid_model::<crate::tests::broken::DividesByZero>();
        let Err(fault) = entry.build(&defaults(&entry), None, None) else {
            panic!("the broken model should not have built");
        };
        assert_eq!(fault.during, BUILDING);
        assert!(fault.to_string().contains("divide by zero"), "{fault}");
    }

    /// The panic an author is most likely to write lands in `step_cell`, which the engine runs on
    /// a rayon worker. Rayon re-raises it on the sim thread without running the panic hook again,
    /// so the location has to survive the hop or the modal loses it for the common case.
    #[test]
    fn a_kernel_that_panics_mid_step_keeps_its_location() {
        use henad_compute::fault::{FaultKind, STEPPING, install_panic_hook};

        install_panic_hook();
        let entry = register_grid_model::<crate::tests::broken::DividesByZeroMidStep>();
        let ModelState::Cpu(mut state) = build(&entry, &defaults(&entry), None) else {
            panic!("a GridModel registers as a CPU entry");
        };
        let outcome: Result<(), _> = catching(STEPPING, || state.step());
        let Err(fault) = outcome else {
            panic!("the broken kernel should not have stepped");
        };
        let FaultKind::Panic { location, .. } = &fault.kind else {
            panic!("expected a panic fault, got {fault:?}");
        };
        let location = location.as_deref().expect("a kernel panic must carry its location");
        assert!(location.contains("broken.rs"), "{location}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_model_entry_can_be_shared_between_threads() {
        fn assert_send_and_sync<T: Send + Sync>() {}
        assert_send_and_sync::<ModelEntry>();
    }

    #[test]
    fn registry_without_gpu_context_offers_no_gpu_models() {
        let entries = model_registry(None);
        assert!(
            !entries
                .iter()
                .any(|e| e.id() == "gpu_game_of_life" || e.id() == "gpu_sir"),
            "a GPU model must not appear in the dropdown when there is no device to run it on"
        );
        assert!(
            entries.iter().any(|e| e.id() == "game_of_life"),
            "CPU models must still be registered without a GPU context"
        );
    }

    /// Building every GPU model on a baseline device is what makes "runs on a stock WebGPU
    /// device" a fact rather than an argument: the engine asserts each pass against the device's
    /// own `max_storage_buffers_per_shader_stage`, which is 8 here.
    #[test]
    fn every_gpu_model_builds_on_a_baseline_device() {
        let Some(ctx) = device() else {
            log::warn!("skipping every_gpu_model_builds_on_a_baseline_device: no adapter");
            return;
        };
        let entries = all_entries(Some(&ctx));
        for entry in entries.iter().filter(|entry| is_gpu(entry)) {
            let params = defaults(entry);
            // The two pin each other: under-report a pass and the build fails, over-report one
            // and the assert does.
            let _built = build(entry, &params, Some(&ctx));
            assert!(
                entry.shortfalls(&params, &wgpu::Limits::default()).is_empty(),
                "{}: builds on a baseline device but its declared demand says it should not: {:?}",
                entry.id(),
                entry.shortfalls(&params, &wgpu::Limits::default())
            );
        }
    }

    /// An oversized submission stops running with no error and no panic, leaving the tick counter
    /// advanced and every readback zero. How many steps that takes depends on the passes a model
    /// records per step, hence every model rather than one.
    #[test]
    fn a_full_submission_executes_every_step() {
        let Some(ctx) = crate::tests::support::headless_context("submission_ceiling_device", wgpu::Features::empty())
        else {
            log::warn!("skipping a_full_submission_executes_every_step: no adapter");
            return;
        };

        for entry in model_registry(Some(ctx.clone())) {
            let ModelState::Gpu(mut state) = build(&entry, &defaults(&entry), Some(&ctx)) else {
                continue;
            };
            let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("henad_submission_ceiling"),
            });
            state.encode_steps(&mut encoder, MAX_STEPS_PER_SUBMISSION, None);
            state.encode_snapshot_passes(&mut encoder);
            ctx.queue.submit(Some(encoder.finish()));
            state.begin_stats_readback();
            state.poll_stats_readback(&ctx.device, true);

            assert_eq!(
                state.tick(),
                u64::from(MAX_STEPS_PER_SUBMISSION),
                "{}: tick after one full submission",
                entry.id()
            );
            assert!(
                state.stats().iter().any(|stat| stat.value.scalar() != 0.0),
                "{}: every stat read back zero after a submission of {MAX_STEPS_PER_SUBMISSION} steps, which is what a dropped submission looks like",
                entry.id()
            );
        }
    }

    /// A slice sampled through the stats passes alone reads back what the snapshot passes do, display included.
    #[test]
    fn a_sampled_slice_reads_back_what_a_snapshot_does() {
        let Some(ctx) = crate::tests::support::headless_context("sampled_slice_device", wgpu::Features::empty()) else {
            log::warn!("skipping a_sampled_slice_reads_back_what_a_snapshot_does: no adapter");
            return;
        };

        for entry in model_registry(Some(ctx.clone())) {
            let ModelState::Gpu(mut state) = build(&entry, &defaults(&entry), Some(&ctx)) else {
                continue;
            };
            for count in [0, 17, MAX_STEPS_PER_SUBMISSION] {
                let tick = state.tick() + u64::from(count);
                let submission = stepping::submit_slice(&mut *state, &ctx, count, true);
                assert!(
                    state.stats_readback_pending(),
                    "{}: a sampled slice begins its readback",
                    entry.id()
                );
                stepping::await_submission(&ctx, submission).expect("the slice runs");
                assert_eq!(
                    state.poll_stats_readback(&ctx.device, false),
                    StatsPoll::Landed,
                    "{}: the readback of a finished slice lands on the next poll",
                    entry.id()
                );
                assert_eq!(state.tick(), tick, "{}: tick after a slice of {count}", entry.id());
                let sliced = state.stats();
                let snapshot = stepping::sample_stats(&mut *state, &ctx);
                assert_eq!(
                    format!("{sliced:?}"),
                    format!("{snapshot:?}"),
                    "{}: stats at tick {tick}",
                    entry.id()
                );
            }
            assert!(ctx.faults.take().is_none(), "{}: the device raised a fault", entry.id());
        }
    }

    /// The app asks every frame, before building, so a missing capacity is a panic on the UI
    /// thread.
    #[test]
    fn every_gpu_entry_reports_its_capacity() {
        let baseline = wgpu::Limits::default();
        let models = crate::example_models();
        for entry in models.iter().filter(|entry| is_gpu(entry)) {
            let demand = entry
                .demand(&defaults(entry), &baseline)
                .expect("a GPU entry declares its capacity");
            assert!(demand.bytes() > 0, "{}: a GPU model allocates something", entry.id());
        }
        for entry in models.iter().filter(|entry| !is_gpu(entry)) {
            assert!(
                entry.demand(&defaults(entry), &baseline).is_none(),
                "{}: a CPU model has no device demand",
                entry.id()
            );
        }
    }

    /// The device a host requests reads the set's needs, so an entry that declares fewer storage
    /// buffers than its widest pass binds builds on a device too narrow for it.
    #[test]
    fn every_gpu_entry_needs_the_bindings_its_widest_pass_binds() {
        let models = crate::example_models();
        let mut widest_overall = 0;
        for entry in models.iter().filter(|entry| is_gpu(entry)) {
            let demand = entry
                .demand(&defaults(entry), &wgpu::Limits::default())
                .expect("a GPU entry declares its capacity");
            let widest = demand.passes.iter().map(|pass| pass.storage).max().unwrap_or(0);
            let needs = entry.gpu_needs().expect("a GPU entry declares its needs");
            assert_eq!(needs.storage_buffers(), widest, "{}: declared needs", entry.id());
            widest_overall = widest_overall.max(widest);
        }
        assert_eq!(models.gpu_needs().storage_buffers(), widest_overall);
    }

    /// Reported, not built. Otherwise the Build button hands wgpu a bind group it rejects.
    #[test]
    fn a_model_too_large_for_the_device_is_reported() {
        let models = crate::example_models();
        let entry = models.get("gpu_sir").expect("gpu_sir is an example model");
        // Baseline limits, so this is issue #9's 6000x6000 case rather than the machine's.
        let baseline = wgpu::Limits::default();
        let mut params = defaults(entry);
        params[0] = ParamValue::U32(6000);
        params[1] = ParamValue::U32(6000);

        let found = entry.shortfalls(&params, &baseline);
        assert!(
            found.iter().any(|s| s.contains("gpu_sir_buffer0")),
            "the over-budget state buffer must be named: {found:?}"
        );
        assert!(
            entry.shortfalls(&defaults(entry), &baseline).is_empty(),
            "the default params fit a baseline device"
        );
    }

    #[test]
    fn every_example_model_joins_the_set() {
        let models = crate::example_models();
        assert_eq!(models.len(), 10);
        for entry in &models {
            assert_eq!(entry.source().package(), "henad-models", "{}", entry.id());
            assert!(
                entry.source().type_path().starts_with("henad_models::"),
                "{}",
                entry.id()
            );
        }
    }

    /// A host that takes one model from the example set records henad-models as its source, so a later change to its
    /// kernel there still shows in the host's results.
    #[test]
    fn an_example_entry_inserted_into_another_set_keeps_its_source() {
        use henad_compute::entry::ModelSet;
        use henad_core::provenance::BuildInfo;

        let host = BuildInfo::__from_env("host", "1.0.0", None, None, None, None, false);
        let sir = crate::example_models()
            .get("sir")
            .expect("sir is an example model")
            .clone();
        let mut models = ModelSet::new(host);
        models.insert(sir).expect("the host's set holds no sir yet");
        let source = models.get("sir").expect("sir joined the set").source();
        assert_eq!(
            (source.package(), source.version()),
            ("henad-models", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(source.type_path(), "henad_models::sir::SirGridModel");
    }

    /// A host without a device can still list a GPU model. Building one then has to come back as a fault, never as a
    /// panic.
    #[test]
    fn a_gpu_entry_refuses_to_build_without_a_device() {
        let models = crate::example_models();
        for entry in models.iter().filter(|entry| is_gpu(entry)) {
            let Err(fault) = entry.build(&defaults(entry), None, None) else {
                panic!("{} built with no device", entry.id());
            };
            assert_eq!(fault.during, BUILDING, "{}", entry.id());
            assert!(
                matches!(
                    models.lookup(entry.id(), None),
                    Err(henad_compute::entry::ModelLookupError::NeedsGpu { .. })
                ),
                "{}",
                entry.id()
            );
            assert!(
                matches!(fault.kind, henad_compute::fault::FaultKind::Refused(_)),
                "{}: {fault:?}",
                entry.id()
            );
        }
    }
}
