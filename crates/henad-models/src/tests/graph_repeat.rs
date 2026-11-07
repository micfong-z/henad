//! `repeat_trials.toml`, the grid file that exercises `region.repeat`, on a device.
//!
//! Each tick every cell makes `trials` Bernoulli draws at probability `p` and keeps whether any hit, so the share of
//! hits is `1 - (1 - p)^trials`. The device's tick equals the host evaluator's run of the same pass, and a copy of the
//! file with a larger bound is refused by the submission budget at a large grid before anything is built.

use henad_compute::gpu::wgpu;
use henad_core::params::ParamValue;
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};
use henad_graph::GraphFile;
use henad_graph::host::{HostEvaluator, HostLanes, HostWorld, RunContext};

use super::graph_grid_parity::{build_gpu, snapshot, stat, step};
use super::lanes::GraphLanes;

/// Returns the text of `tests/fixtures/graphs/repeat_trials.toml`.
fn trials_text() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graphs/repeat_trials.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Returns the parameters of the file at `side` by `side` cells.
fn trials_params(side: u32, trials: u32, p: f32) -> [ParamValue; 4] {
    [
        ParamValue::U32(side),
        ParamValue::U32(side),
        ParamValue::U32(trials),
        ParamValue::F32(p),
    ]
}

/// Half-width of the acceptance band, in standard deviations of a binomial proportion.
const SIGMAS: f64 = 5.0;

/// The share of cells that hit each tick on the device is `1 - (1 - p)^trials` within five standard deviations, and
/// exactly 0 or 1 where the closed form is. The trials are independent across ticks, and a count past the bound would
/// be clamped, which the parameter's own bound prevents.
#[test]
fn repeated_trials_hit_at_the_closed_form_rate() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping repeated_trials_hit_at_the_closed_form_rate: no adapter");
        return;
    };
    let entry = ModelFile::parse(&trials_text())
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("repeat_trials.toml: {error}"));
    let lanes = GraphLanes::of_text(&trials_text());
    let side = 512u32;
    let cells = f64::from(side * side);
    let cases = [
        (4, 0.1),
        (1, 0.1),
        (3, 0.25),
        (2, 0.5),
        (4, 0.03),
        (0, 0.5),
        (2, 1.0),
        (4, 0.0),
    ];
    for (trials, p) in cases {
        let expected = 1.0 - (1.0 - f64::from(p)).powi(trials);
        let allowed = SIGMAS * (expected * (1.0 - expected) / cells).sqrt();
        let mut state = build_gpu(&entry, &trials_params(side, trials as u32, p), Some(17), &ctx);
        let mut previous = Vec::new();
        for tick in 0..4 {
            step(&ctx, &mut *state, 1);
            snapshot(&ctx, &mut *state);
            let hits = stat(&state.stats(), "Hits");
            let current = lanes.read(&*state, "outcome");
            assert_eq!(
                hits,
                current.iter().map(|&cell| f64::from(cell)).sum::<f64>(),
                "the count of {trials} trials at {p}"
            );
            let observed = hits / cells;
            assert!(
                (observed - expected).abs() <= allowed,
                "{trials} trials at {p}, tick {tick}: observed {observed:.6}, expected {expected:.6}, \
                 allowed {allowed:.6}"
            );
            if (0.0..1.0).contains(&expected) && expected > 0.0 {
                assert_ne!(
                    current, previous,
                    "{trials} trials at {p}: tick {tick} drew as the tick before"
                );
            }
            previous = current;
        }
    }
}

/// Each tick of `repeat_trials.toml` on the device equals the host evaluator's run of its tick pass, cell for cell, at
/// two seeds.
#[test]
fn repeated_trials_on_the_device_equal_the_host() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping repeated_trials_on_the_device_equal_the_host: no adapter");
        return;
    };
    let text = trials_text();
    let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{error}"));
    let entry = ModelFile::parse(&text)
        .and_then(|model_file| model_file.entry())
        .unwrap_or_else(|error| panic!("repeat_trials.toml: {error}"));
    let device_lanes = GraphLanes::of(&file);
    let (width, height) = (61u32, 37u32);
    let own = [ParamValue::U32(3), ParamValue::F32(0.2)];
    for seed in [Some(2), None] {
        let params = [
            ParamValue::U32(width),
            ParamValue::U32(height),
            own[0].clone(),
            own[1].clone(),
        ];
        let mut state = build_gpu(&entry, &params, seed, &ctx);
        let world = HostWorld::Grid {
            width,
            height,
            boundary: henad_core::authoring::primitives::space::Boundary::Torus,
        };
        let mut lanes = HostLanes::new(evaluator.document(), world).expect("the lanes");
        for tick in 0..6u32 {
            step(&ctx, &mut *state, 1);
            let mut stream = 0;
            evaluator
                .run_pass("step", &mut lanes, &own, &RunContext::tick(seed, tick), &mut stream)
                .unwrap_or_else(|error| panic!("{error}"));
            let host = lanes.column("outcome").expect("the lane").words();
            assert!(
                device_lanes.read(&*state, "outcome") == host,
                "the cells differ at tick {tick}, seed {seed:?}"
            );
        }
    }
}

/// A copy of `repeat_trials.toml` with a bound of 140 trials costs under 1000 units a cell, far within the limit of
/// 65536 a pass, and checks and compiles. The submission budget admits it at 2048 by 2048 cells and refuses it at 4096
/// by 4096 with a line naming its step pass, and a build at that size fails before the engine allocates or seeds.
#[test]
fn a_repeat_within_the_pass_limit_is_refused_by_the_submission_budget_at_a_large_grid() {
    let text = trials_text()
        .replace("config.max = 4\n", "config.max = 140\n")
        .replace("max = 4\ndefault = 4\n", "max = 140\ndefault = 140\n");
    let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    let report = file.check();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let step_cost = report.passes[0].cost;
    assert!(step_cost > 980 && step_cost < 1000, "the step costs {step_cost}");
    let entry = ModelFile::parse(&text)
        .and_then(|model_file| model_file.entry())
        .unwrap_or_else(|error| panic!("{error}"));
    let limits = wgpu::Limits::default();
    assert!(
        entry.shortfalls(&trials_params(2048, 140, 0.1), &limits).is_empty(),
        "2048 by 2048 fits"
    );
    let refused = entry.shortfalls(&trials_params(4096, 140, 0.1), &limits);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("past the budget") && refused[0].contains("pass 'node_repeat_trials_step' is the heaviest"),
        "{refused:?}"
    );
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the build of a_repeat_within_the_pass_limit_is_refused: no adapter");
        return;
    };
    match entry.build(&trials_params(4096, 140, 0.1), None, Some(&ctx)) {
        Ok(_) => panic!("the build passed the budget"),
        Err(fault) => assert!(fault.to_string().contains("past the budget"), "{fault}"),
    }
}
