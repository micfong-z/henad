//! The registry tests: each example model's declarations checked against what its state does, through the testing
//! kit, and the guards that hold for the example models alone.

use henad_compute::entry::{ModelState, register_grid_model};
use henad_compute::fault::{BUILDING, catching, install_panic_hook};
use henad_core::params::ParamValue;
use henad_explore::testing::{
    CheckSettings, ModelCheck, ModelReport, SkipReason, TestDeviceRequest, check_model_set, headless_test_device,
};

use henad_compute::entry::ModelEntry;
use henad_core::metadata::Backend;

// --8<-- [start:kit]
/// Settings of the kit over the example models, on a baseline device when this machine gives one, and whether it gave
/// one.
///
/// The device asks for `Limits::default()`, so a GPU model that only fits a raised limit fails to build here. Every
/// example model is meant to run on a stock WebGPU device.
fn kit_settings() -> (CheckSettings, bool) {
    let settings = CheckSettings::default();
    if let Some(device) = headless_test_device(&TestDeviceRequest::baseline()) {
        (settings.gpu(device), true)
    } else {
        log::warn!("checking the example models without their GPU checks: no adapter");
        (settings, false)
    }
}

/// Checks every example model once, asserting that each passes and skips only what its backend, its declared
/// replay or a missing device rules out.
#[test]
fn the_example_models_conform() {
    let (settings, has_device) = kit_settings();
    install_panic_hook();
    let models = crate::example_models();
    let report = check_model_set(&models, &settings);
    report.assert_passed();
    for (entry, model_report) in models.iter().zip(report.reports()) {
        assert_skips_only_what_it_declares(entry, model_report, has_device);
    }
}
// --8<-- [end:kit]

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
fn build(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&henad_compute::gpu::GpuContext>) -> ModelState {
    entry
        .build(values, None, gpu)
        .unwrap_or_else(|fault| panic!("{}: {fault}", entry.id()))
}

/// Asserts that `report` skips only the checks the backend of `entry`, its declared replay or a missing device rule
/// out, and that a CPU model's step split into more than one job.
fn assert_skips_only_what_it_declares(entry: &ModelEntry, report: &ModelReport, has_device: bool) {
    let skipped: Vec<(ModelCheck, SkipReason)> = report
        .skipped()
        .iter()
        .map(|skipped| (skipped.check(), skipped.reason().clone()))
        .collect();
    let expected: Vec<(ModelCheck, SkipReason)> = if is_gpu(entry) {
        ModelCheck::ALL
            .iter()
            .copied()
            .filter_map(|check| match check {
                ModelCheck::ThreadCount => Some((check, SkipReason::OtherBackend)),
                ModelCheck::SameSeed | ModelCheck::SeedSensitivity | ModelCheck::SamplingCadence
                    if entry.id() == "gpu_boids" =>
                {
                    Some((check, SkipReason::InexactReplay))
                }
                ModelCheck::ApplyModes
                | ModelCheck::Views
                | ModelCheck::ParallelJobs
                | ModelCheck::Actions
                | ModelCheck::StatCount
                | ModelCheck::SameSeed
                | ModelCheck::SeedSensitivity
                | ModelCheck::SamplingCadence
                | ModelCheck::BaselineBuild
                | ModelCheck::FullSubmission
                | ModelCheck::SampledSlice
                    if !has_device =>
                {
                    Some((check, SkipReason::NoDevice))
                }
                _ => None,
            })
            .collect()
    } else {
        [
            ModelCheck::DefaultsFit,
            ModelCheck::BaselineBuild,
            ModelCheck::FullSubmission,
            ModelCheck::SampledSlice,
        ]
        .map(|check| (check, SkipReason::OtherBackend))
        .to_vec()
    };
    assert_eq!(skipped, expected, "{report}");
    if !is_gpu(entry) {
        assert!(
            report.thread_count_jobs().is_some_and(|jobs| jobs > 1),
            "{}: a step splits into {:?} jobs",
            entry.id(),
            report.thread_count_jobs()
        );
    }
}

/// The GPU checks pick a model by its declared backend, never by its id.
#[test]
fn the_example_gpu_models_are_the_gpu_backend_entries() {
    let models = crate::example_models();
    let gpu = ["gpu_game_of_life", "gpu_sir", "gpu_boids", "gpu_ants"];
    let found: Vec<&str> = models
        .iter()
        .filter(|entry| is_gpu(entry))
        .map(ModelEntry::id)
        .collect();
    assert_eq!(found, gpu, "the GPU models are picked by backend");
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

/// The index tables in a GPU entry's demand match the hash grid the engine builds, on both sides of a cell size of 1.
#[test]
fn gpu_boids_sizes_its_index_tables_to_the_hash_grid() {
    use henad_core::Extent;
    use henad_core::spatial_hash::HashGrid;

    let models = crate::example_models();
    let entry = models.get("gpu_boids").expect("gpu_boids is an example model");
    let range = entry
        .param_index("visual_range")
        .expect("gpu_boids declares visual_range");
    for visual_range in [50.0, 1.0] {
        let mut params = defaults(entry);
        params[range] = ParamValue::F32(visual_range);
        let demand = entry
            .demand(&params, &wgpu::Limits::default())
            .expect("a GPU entry declares its capacity");
        let grid = HashGrid::new(Extent { w: 1000.0, h: 1000.0 }, visual_range / 3.0);
        let counts = demand
            .buffers
            .iter()
            .find(|alloc| alloc.label == "gpu_boids_hash_counts")
            .expect("gpu_boids declares an index");
        assert_eq!(
            counts.bytes,
            (u64::from(grid.num_cells()) + 1) * 4,
            "visual_range {visual_range}"
        );
    }
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
    // `type_name` promises no exact form, so the path is matched by its crate and its type.
    let path = source.type_path();
    assert!(
        path.starts_with("henad_models::") && path.ends_with("::SirGridModel"),
        "{path}"
    );
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
